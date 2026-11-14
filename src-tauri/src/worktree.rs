//! 独立工作区（v0.7 Phase 10）：发任务时可选，任务在项目的一个 git worktree 里做，主文件夹不动；
//! 做完站主看过改动，一键搬回（APPLY）或者丢掉（DISCARD）。
//!
//! - **建**：`git worktree add --detach <Peck 数据目录>/worktrees/<项目哈希>/<任务号> HEAD`（不建分支、不动主文件夹的
//!   index、commit、stash；只在用户仓库的 `.git/worktrees/` 里多一份登记，搬回或丢掉时删掉），再把主文件夹里没提交的
//!   改动（改过的、新建没忽略的、删掉的）照样搬进去，所以任务开始时两边一样。
//! - **快照**：独立工作区的任务前快照、做完的快照都存在项目自己的影子仓库里（同一个 `shadow/<项目哈希>/`），
//!   CHANGES 照常看 Diff。
//! - **搬回**：做完到现在独立工作区改了哪些文件；主文件夹里这些文件要和任务开始时一样（不然是冲突，默认不搬，
//!   站主可以选「照样搬」覆盖）；搬完删掉独立工作区，任务记录换成主文件夹，撤销整个任务照常能用。
//! - **丢掉**：删掉独立工作区，改动只在 CHANGES 的历史 Diff 里。

use crate::events::RunStatus;
use crate::snapshot::{resolve_shadow_dir, ShadowRepo};
use crate::state::AppState;
use crate::store::{now_ms, RunRecord};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

/// 任务记录里的独立工作区。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeState {
    /// 独立工作区的文件夹。
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub discarded_at: Option<i64>,
}

impl WorktreeState {
    pub fn pending(&self) -> bool {
        self.applied_at.is_none() && self.discarded_at.is_none()
    }
}

fn git(main: &Path) -> Command {
    let bin = if Path::new("/usr/bin/git").exists() { "/usr/bin/git" } else { "git" };
    let mut c = Command::new(bin);
    // 不跑仓库的钩子：`worktree add` 会触发 post-checkout，而钩子文件（比如 husky 的）是模型在沙箱里就能改的项目文件，
    // 这里却是在 Peck 自己的进程里、沙箱外跑。
    c.args(["-c", "core.hooksPath=/dev/null"])
        .current_dir(main)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    c
}

/// 主文件夹是不是 git 仓库的根（独立工作区只给 git 项目）。
pub fn available(main: &Path) -> bool {
    main.join(".git").exists()
}

/// 同一个任务里上一次在独立工作区做的还没搬回、没丢掉：接着说的话不知道该在哪做（对话里说写了的文件，项目里没有），先让站主定。
pub fn pending_in_session(db: &crate::store::Store, session_id: &str) -> Result<(), String> {
    let pending = db.runs(session_id).unwrap_or_default().iter().any(|r| r.worktree.as_ref().is_some_and(|w| w.pending()));
    if pending {
        return Err("这个任务上一次在独立工作区里做的还没搬回：先在 CHANGES 里 APPLY 或 DISCARD，再接着说。".into());
    }
    Ok(())
}

/// 这个项目的独立工作区都放这里。
pub fn root_for(main: &Path) -> PathBuf {
    crate::mode::app_data_dir().join("worktrees").join(crate::workspace::workspace_hash(main))
}

fn safe_rel(rel: &str) -> bool {
    let p = Path::new(rel);
    !rel.is_empty()
        && !p.is_absolute()
        && p.components().all(|c| matches!(c, Component::Normal(_)))
        && !p.components().any(|c| c.as_os_str().to_str().is_some_and(|s| s.eq_ignore_ascii_case(".git")))
}

/// 建一个独立工作区（`at` 是要放的文件夹，还不存在），把主文件夹没提交的改动搬进去。
pub fn create(main: &Path, at: &Path) -> Result<(), String> {
    if !available(main) {
        return Err("这个项目不是 git 仓库，开不了独立工作区。".into());
    }
    let head = git(main).args(["rev-parse", "--verify", "HEAD"]).output().map_err(|e| format!("git 起不来：{e}"))?;
    if !head.status.success() {
        return Err("这个仓库还没有提交，开不了独立工作区。".into());
    }
    if at.exists() {
        return Err("独立工作区的文件夹已经有了。".into());
    }
    std::fs::create_dir_all(at.parent().unwrap_or(at)).map_err(|e| format!("建不了独立工作区：{e}"))?;
    let out = git(main)
        .args(["worktree", "add", "--detach", "--quiet"])
        .arg(at)
        .arg("HEAD")
        .output()
        .map_err(|e| format!("git 起不来：{e}"))?;
    if !out.status.success() {
        return Err(format!("建不了独立工作区：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    // 没提交的改动照搬：改过的、新建的（没被忽略的）复制过去，删掉的也删掉。
    let status = git(main)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .map_err(|e| format!("git 起不来：{e}"))?;
    let raw = String::from_utf8_lossy(&status.stdout).into_owned();
    let mut items = raw.split('\0').filter(|s| !s.is_empty());
    while let Some(entry) = items.next() {
        if entry.len() < 4 {
            continue;
        }
        let code = &entry[..2];
        let rel = &entry[3..];
        if code.starts_with('R') || code.starts_with('C') {
            // 改名：后面跟着原来的名字；原来的在新的工作区里删掉。
            if let Some(old) = items.next() {
                if code.starts_with('R') && safe_rel(old) {
                    let _ = std::fs::remove_file(at.join(old));
                }
            }
        }
        if !safe_rel(rel) {
            continue;
        }
        let src = main.join(rel);
        let dst = at.join(rel);
        if src.is_file() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("搬不进 {rel}：{e}"))?;
            }
            std::fs::copy(&src, &dst).map_err(|e| format!("搬不进 {rel}：{e}"))?;
        } else if !src.exists() && dst.is_file() {
            std::fs::remove_file(&dst).map_err(|e| format!("删不掉 {rel}：{e}"))?;
        }
    }
    Ok(())
}

/// 独立工作区的影子仓库：项目自己的那一个（树按内容存，两边的 Diff 对得上）。
pub fn shadow_repo(worktree: &Path, main: &Path) -> Result<ShadowRepo, String> {
    let shadow = resolve_shadow_dir(&crate::workspace::normalize_workspace_path(main));
    ShadowRepo::with_shadow_dir(worktree, &shadow)
}

/// 删掉独立工作区（连同用户仓库里的登记）。git 删不掉时自己删文件夹和那一条登记，不 `prune`（会顺手清掉站主自己的）。
pub fn remove(main: &Path, worktree: &Path) {
    let ok = git(main)
        .args(["worktree", "remove", "--force"])
        .arg(worktree)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok {
        return;
    }
    let _ = std::fs::remove_dir_all(worktree);
    let ours = worktree.join(".git");
    let Ok(entries) = std::fs::read_dir(main.join(".git").join("worktrees")) else { return };
    for e in entries.flatten() {
        let points = std::fs::read_to_string(e.path().join("gitdir")).map(|t| PathBuf::from(t.trim())).ok();
        if points.is_some_and(|p| p == ours || p.canonicalize().ok() == ours.canonicalize().ok()) {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    /// 搬回去的文件。
    pub applied: Vec<String>,
    /// 主文件夹里这些文件在任务开始以后也变了（没搬）。
    pub conflicts: Vec<String>,
    /// 做完的样子（快照）。
    #[serde(skip)]
    pub tree: String,
}

/// 把独立工作区里这个任务的改动搬回主文件夹（`repo` 是独立工作区的影子仓库）。有冲突、没让照样搬时什么都不动。
pub fn apply(repo: &ShadowRepo, main: &Path, base_tree: &str, run_id: &str, force: bool) -> Result<ApplyResult, String> {
    let worktree = repo.workspace_path.clone();
    let now = repo.create_snapshot(&format!("{run_id}-worktree-apply"))?;
    let changes = repo.diff_changeset(base_tree, &now, "")?;
    let mut result = ApplyResult { tree: now, ..Default::default() };
    let main_repo = ShadowRepo::with_shadow_dir(main, &repo.shadow_dir)?;
    for f in &changes.files {
        if !safe_rel(&f.path) {
            continue;
        }
        // 按 git 的口径比（换行、过滤器照 .gitattributes），免得 CRLF、LFS 这类文件总算冲突。
        if main_repo.blob_id_of_file(&f.path) != repo.blob_id_at(base_tree, &f.path) {
            result.conflicts.push(f.path.clone());
        }
    }
    if !result.conflicts.is_empty() && !force {
        return Ok(result);
    }
    // 先都看一遍再动手：读不到的、目标是个文件夹的，一个都不搬。
    for f in changes.files.iter().filter(|f| safe_rel(&f.path)) {
        if f.kind != "delete" && !worktree.join(&f.path).is_file() {
            return Err(format!("独立工作区里读不到 {}，没有搬。", f.path));
        }
        if main.join(&f.path).is_dir() {
            return Err(format!("项目里的 {} 现在是个文件夹，没有搬。", f.path));
        }
    }
    for f in changes.files.iter().filter(|f| safe_rel(&f.path)) {
        let dst = main.join(&f.path);
        if f.kind == "delete" {
            if dst.is_file() {
                std::fs::remove_file(&dst).map_err(|e| format!("删不掉 {}：{e}", f.path))?;
            }
        } else {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| format!("写不进 {}：{e}", f.path))?;
            }
            // 复制（带上可执行位这些权限）到临时文件再换上。
            let tmp = dst.with_extension("peck-tmp");
            std::fs::copy(worktree.join(&f.path), &tmp).map_err(|e| format!("写不进 {}：{e}", f.path))?;
            std::fs::rename(&tmp, &dst).map_err(|e| format!("写不进 {}：{e}", f.path))?;
        }
        result.applied.push(f.path.clone());
    }
    Ok(result)
}

// ───────────────────────── IPC ─────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeApply {
    pub applied: Vec<String>,
    pub conflicts: Vec<String>,
    pub record: RunRecord,
}

fn pending_run(state: &AppState, run_id: &str) -> Result<(RunRecord, WorktreeState, PathBuf), String> {
    let db = state.store.lock().map_err(|_| "读不到任务记录。")?;
    let record = db.run(run_id).map_err(|_| "没找到这个任务。")?;
    if record.status == RunStatus::Running {
        return Err("任务还在跑，跑完再搬。".into());
    }
    let wt = record.worktree.clone().ok_or("这个任务不在独立工作区里。")?;
    if !wt.pending() {
        return Err(if wt.applied_at.is_some() { "已经搬回去了。" } else { "已经丢掉了。" }.into());
    }
    let ws = db.workspace_for_session(&record.session_id).map_err(|e| e.to_string())?;
    let main = PathBuf::from(&ws.path);
    if !main.is_dir() {
        return Err("项目文件夹不在了。".into());
    }
    let busy = state.active_runs.lock().map(|a| a.values().any(|r| r.touches(&[main.clone()]))).unwrap_or(false);
    if busy {
        return Err("这个项目有任务在跑，等它做完再搬。".into());
    }
    Ok((record, wt, main))
}

/// 搬回主文件夹。`force` 为真时冲突的文件也照样覆盖。
#[tauri::command]
pub async fn worktree_apply(run_id: String, force: Option<bool>, state: tauri::State<'_, AppState>) -> Result<WorktreeApply, String> {
    let (mut record, wt, main) = pending_run(&state, &run_id)?;
    let snapshot = {
        let db = state.store.lock().map_err(|_| "读不到任务记录。")?;
        db.snapshot(&run_id).map_err(|e| e.to_string())?.ok_or("这个任务没有任务前快照，搬不回去。")?
    };
    crate::snapshot::restore_snapshot(snapshot.clone());
    let worktree = PathBuf::from(&wt.path);
    let (m, w, id) = (main.clone(), worktree.clone(), run_id.clone());
    let force = force.unwrap_or(false);
    // 搬之前、搬之后各拍一份项目文件夹：撤销整个任务就回到搬之前（照样搬时覆盖掉的站主的改动也回得来）。
    let (result, trees) = tokio::task::spawn_blocking(move || -> Result<(ApplyResult, Option<(String, String)>), String> {
        let repo = shadow_repo(&w, &m)?;
        let main_repo = ShadowRepo::with_shadow_dir(&m, &repo.shadow_dir)?;
        let before = main_repo.create_snapshot(&format!("{id}-before-apply"))?;
        let result = apply(&repo, &m, &snapshot.base_tree, &id, force)?;
        if !result.conflicts.is_empty() && !force {
            return Ok((result, None));
        }
        let after = main_repo.create_snapshot(&format!("{id}-after-apply"))?;
        Ok((result, Some((before, after))))
    })
    .await
    .map_err(|e| e.to_string())??;
    if let Some((before, after)) = trees {
        remove(&main, &worktree);
        crate::snapshot::rebase_record(&run_id, &main, Some(before), Some(after));
        record.worktree = Some(WorktreeState { applied_at: Some(now_ms()), ..wt });
        state.store.lock().map_err(|_| "存不了任务记录。")?.save_run(&record).map_err(|e| e.to_string())?;
    }
    Ok(WorktreeApply { applied: result.applied, conflicts: result.conflicts, record })
}

/// 丢掉独立工作区（改动只留在 CHANGES 的历史 Diff 里）。
#[tauri::command]
pub async fn worktree_discard(run_id: String, state: tauri::State<'_, AppState>) -> Result<RunRecord, String> {
    let (mut record, wt, main) = pending_run(&state, &run_id)?;
    let worktree = PathBuf::from(&wt.path);
    let m = main.clone();
    tokio::task::spawn_blocking(move || remove(&m, &worktree)).await.map_err(|e| e.to_string())?;
    // 树都在项目的影子仓库里：记录换成项目文件夹，CHANGES 的历史 Diff 照样能看。
    if let Ok(Some(snapshot)) = state.store.lock().map_err(|_| ()).and_then(|db| db.snapshot(&run_id).map_err(|_| ())) {
        crate::snapshot::restore_snapshot(snapshot);
    }
    crate::snapshot::rebase_record(&run_id, &main, None, None);
    record.worktree = Some(WorktreeState { discarded_at: Some(now_ms()), ..wt });
    state.store.lock().map_err(|_| "存不了任务记录。")?.save_run(&record).map_err(|e| e.to_string())?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let out = git(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@e")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@e")
            .output()
            .unwrap();
        assert!(out.status.success(), "{:?}: {}", args, String::from_utf8_lossy(&out.stderr));
    }

    fn repo() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("peck-wt-{}", uuid::Uuid::new_v4()));
        let main = root.join("main");
        std::fs::create_dir_all(main.join("src")).unwrap();
        std::fs::write(main.join("src/a.txt"), "a1\n").unwrap();
        std::fs::write(main.join("gone.txt"), "x\n").unwrap();
        std::fs::write(main.join(".gitignore"), "target/\n").unwrap();
        sh(&main, &["init", "-q", "-b", "main"]);
        sh(&main, &["add", "."]);
        sh(&main, &["commit", "-qm", "first"]);
        (root, main.canonicalize().unwrap())
    }

    #[test]
    fn a_worktree_starts_like_the_main_folder_and_applies_back() {
        let (root, main) = repo();
        // 没提交的改动：改一个、新建一个、删一个、一个被忽略的
        std::fs::write(main.join("src/a.txt"), "a2\n").unwrap();
        std::fs::write(main.join("new.txt"), "n\n").unwrap();
        std::fs::remove_file(main.join("gone.txt")).unwrap();
        std::fs::create_dir_all(main.join("target")).unwrap();
        std::fs::write(main.join("target/big.bin"), "ignored").unwrap();
        let index_before = std::fs::read(main.join(".git/index")).unwrap();
        let wt = root.join("wt/run-1");
        create(&main, &wt).unwrap();
        assert_eq!(std::fs::read_to_string(wt.join("src/a.txt")).unwrap(), "a2\n");
        assert!(wt.join("new.txt").is_file() && !wt.join("gone.txt").exists() && !wt.join("target").exists());
        assert_eq!(std::fs::read(main.join(".git/index")).unwrap(), index_before, "主文件夹的 index 不动");

        let shadow = root.join("shadow");
        let repo = ShadowRepo::with_shadow_dir(&wt, &shadow).unwrap();
        let base = repo.create_snapshot("base").unwrap();
        // 任务在独立工作区里改
        std::fs::write(wt.join("src/a.txt"), "a3\n").unwrap();
        std::fs::write(wt.join("src/b.txt"), "b\n").unwrap();
        std::fs::remove_file(wt.join("new.txt")).unwrap();
        assert_eq!(std::fs::read_to_string(main.join("src/a.txt")).unwrap(), "a2\n", "主文件夹不动");

        let r = apply(&repo, &main, &base, "run-1", false).unwrap();
        assert!(r.conflicts.is_empty(), "{:?}", r.conflicts);
        assert_eq!(r.applied.len(), 3);
        assert_eq!(std::fs::read_to_string(main.join("src/a.txt")).unwrap(), "a3\n");
        assert!(main.join("src/b.txt").is_file() && !main.join("new.txt").exists());
        remove(&main, &wt);
        assert!(!wt.exists());
        let list = git(&main).args(["worktree", "list"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&list.stdout).lines().count(), 1, "登记也删了");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn apply_anyway_keeps_modes_and_undo_brings_back_the_users_edit() {
        let (root, main) = repo();
        let wt = root.join("wt/run-905001");
        create(&main, &wt).unwrap();
        let shadow = root.join("shadow");
        let repo = ShadowRepo::with_shadow_dir(&wt, &shadow).unwrap();
        let base = repo.create_snapshot("base").unwrap();
        crate::snapshot::record_run_base("run-905001", &repo, base.clone());
        // 任务改了 a.txt、新建一个能执行的脚本；站主同时在主文件夹里也改了 a.txt（冲突）
        std::fs::write(wt.join("src/a.txt"), "task\n").unwrap();
        std::fs::write(wt.join("run.sh"), "#!/bin/sh\necho hi\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(wt.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(main.join("src/a.txt"), "user\n").unwrap();
        let r = apply(&repo, &main, &base, "run-905001", false).unwrap();
        assert_eq!(r.conflicts, vec!["src/a.txt".to_string()]);
        assert_eq!(std::fs::read_to_string(main.join("src/a.txt")).unwrap(), "user\n", "有冲突、没让照样搬：不动");
        // 照样搬：和 worktree_apply 一样，前后各拍一份主文件夹
        let main_repo = ShadowRepo::with_shadow_dir(&main, &shadow).unwrap();
        let before = main_repo.create_snapshot("before-apply").unwrap();
        apply(&repo, &main, &base, "run-905001", true).unwrap();
        let after = main_repo.create_snapshot("after-apply").unwrap();
        assert_eq!(std::fs::read_to_string(main.join("src/a.txt")).unwrap(), "task\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(main.join("run.sh")).unwrap().permissions().mode() & 0o111, 0o111, "可执行位跟过来");
        }
        crate::snapshot::rebase_record("run-905001", &main, Some(before), Some(after));
        // 撤销整个任务：回到搬之前（站主自己的改动回来，新脚本删掉）
        let record = crate::snapshot::saved_snapshot("run-905001").unwrap();
        crate::snapshot::undo_run(&record).unwrap();
        assert_eq!(std::fs::read_to_string(main.join("src/a.txt")).unwrap(), "user\n");
        assert!(!main.join("run.sh").exists());
        remove(&main, &wt);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn apply_reports_conflicts_and_only_overwrites_when_forced() {
        let (root, main) = repo();
        let wt = root.join("wt/run-2");
        create(&main, &wt).unwrap();
        let repo = ShadowRepo::with_shadow_dir(&wt, &root.join("shadow")).unwrap();
        let base = repo.create_snapshot("base").unwrap();
        std::fs::write(wt.join("src/a.txt"), "from task\n").unwrap();
        std::fs::write(wt.join("ok.txt"), "fine\n").unwrap();
        std::fs::write(main.join("src/a.txt"), "站主自己改的\n").unwrap();
        let r = apply(&repo, &main, &base, "run-2", false).unwrap();
        assert_eq!(r.conflicts, vec!["src/a.txt".to_string()]);
        assert!(r.applied.is_empty() && !main.join("ok.txt").exists(), "有冲突时什么都不动");
        let r = apply(&repo, &main, &base, "run-2", true).unwrap();
        assert_eq!(r.applied.len(), 2);
        assert_eq!(std::fs::read_to_string(main.join("src/a.txt")).unwrap(), "from task\n");
        assert!(main.join("ok.txt").is_file());
        remove(&main, &wt);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn non_git_and_unborn_repos_are_refused() {
        let root = std::env::temp_dir().join(format!("peck-wt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(create(&root, &root.join("wt")).unwrap_err().contains("不是 git"));
        sh(&root, &["init", "-q"]);
        assert!(create(&root, &root.join("wt")).unwrap_err().contains("还没有提交"));
        assert!(!safe_rel("../x") && !safe_rel(".git/config") && !safe_rel("/etc/x") && safe_rel("src/a.txt"));
        let _ = std::fs::remove_dir_all(root);
    }
}
