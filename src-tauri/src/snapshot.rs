use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::events::{ChangeSetSummary, FileChangeItem};
use crate::mode::app_data_dir;
use crate::workspace::{normalize_workspace_path, workspace_hash};

/// Shadow Git bare repository controller.
/// Uses `GIT_DIR=<shadow_dir>` and `GIT_WORK_TREE=<workspace_path>` so that user repository
/// `.git/index`, commit history, and stashes are never touched.
#[derive(Debug, Clone)]
pub struct ShadowRepo {
    pub workspace_path: PathBuf,
    pub shadow_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSnapshotRecord {
    pub run_id: String,
    pub workspace_path: PathBuf,
    pub shadow_dir: PathBuf,
    pub base_tree: String,
    pub latest_tree: String,
    /// 项目里「能改」的其他文件夹（v0.7）：每个一份任务前、做完的快照。改动列表里用绝对路径。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<ExtraSnapshot>,
}

/// 一个其他文件夹的快照（它自己的影子仓库）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExtraSnapshot {
    pub root: PathBuf,
    pub shadow_dir: PathBuf,
    pub base_tree: String,
    pub latest_tree: String,
}

/// 记一个其他文件夹的任务前快照。
pub fn record_run_extra(run_id: &str, repo: &ShadowRepo, base_tree: String) {
    if let Ok(mut guard) = snapshot_registry().lock() {
        if let Some(rec) = guard.get_mut(run_id) {
            rec.extras.retain(|e| e.root != repo.workspace_path);
            rec.extras.push(ExtraSnapshot {
                root: repo.workspace_path.clone(),
                shadow_dir: repo.shadow_dir.clone(),
                base_tree: base_tree.clone(),
                latest_tree: base_tree,
            });
        }
    }
}

/// 记一个其他文件夹做完的快照。
pub fn record_extra_finish(run_id: &str, root: &Path, after_tree: String) {
    if let Ok(mut guard) = snapshot_registry().lock() {
        if let Some(e) = guard.get_mut(run_id).and_then(|rec| rec.extras.iter_mut().find(|e| e.root == root)) {
            e.latest_tree = after_tree;
        }
    }
}

/// 改了别处时，换一下记录里的项目文件夹（独立工作区搬回以后，撤销照项目文件夹来：前后两棵树是搬之前、搬之后）。
pub fn rebase_record(run_id: &str, workspace_path: &Path, base_tree: Option<String>, latest_tree: Option<String>) {
    if let Ok(mut guard) = snapshot_registry().lock() {
        if let Some(rec) = guard.get_mut(run_id) {
            rec.workspace_path = workspace_path.to_path_buf();
            if let Some(t) = base_tree {
                rec.base_tree = t;
            }
            if let Some(t) = latest_tree {
                rec.latest_tree = t;
            }
        }
    }
}

impl ExtraSnapshot {
    /// 绝对路径 → 这个文件夹里的相对路径（不在里面为空）。
    pub fn rel_of(&self, abs: &str) -> Option<String> {
        Path::new(abs).strip_prefix(&self.root).ok().map(|r| r.to_string_lossy().replace('\\', "/")).filter(|r| !r.is_empty())
    }
}

static RUN_SNAPSHOTS: OnceLock<Mutex<HashMap<String, RunSnapshotRecord>>> = OnceLock::new();

pub fn saved_snapshot(run_id: &str) -> Option<RunSnapshotRecord> {
    snapshot_registry().lock().ok()?.get(run_id).cloned()
}

pub fn restore_snapshot(record: RunSnapshotRecord) {
    if let Ok(mut registry) = snapshot_registry().lock() {
        registry.insert(record.run_id.clone(), record);
    }
}

fn snapshot_registry() -> &'static Mutex<HashMap<String, RunSnapshotRecord>> {
    RUN_SNAPSHOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve the default shadow repository directory for a workspace:
/// `<app_data_dir>/shadow/<workspace_md5>/` (falls back to `<temp_dir>/peck-shadow/<workspace_md5>/`
/// if `<app_data_dir>` is not writable in a sandboxed test environment).
pub fn resolve_shadow_dir(workspace_path: &Path) -> PathBuf {
    let hash = workspace_hash(workspace_path);
    if let Ok(custom_root) = std::env::var("PECK_SHADOW_ROOT") {
        let candidate = PathBuf::from(custom_root).join(&hash);
        if fs::create_dir_all(&candidate).is_ok() {
            return candidate;
        }
    }

    let primary = app_data_dir().join("shadow").join(&hash);
    if fs::create_dir_all(&primary).is_ok() {
        return primary;
    }

    let fallback = std::env::temp_dir().join("peck-shadow").join(&hash);
    let _ = fs::create_dir_all(&fallback);
    fallback
}

impl ShadowRepo {
    /// Open or initialize the Shadow Git bare repository at the default `<app_data_dir>/shadow/<workspace_md5>/`.
    pub fn open_or_init(workspace_path: &Path) -> Result<Self, String> {
        let normalized_ws = normalize_workspace_path(workspace_path);
        let shadow_dir = resolve_shadow_dir(&normalized_ws);
        Self::with_shadow_dir(&normalized_ws, &shadow_dir)
    }

    /// Open or initialize a Shadow Git bare repository at an explicit `shadow_dir`.
    pub fn with_shadow_dir(workspace_path: &Path, shadow_dir: &Path) -> Result<Self, String> {
        if !workspace_path.exists() {
            return Err(format!(
                "项目文件夹不在了（{}），读不了它的快照。",
                workspace_path.display()
            ));
        }

        fs::create_dir_all(shadow_dir)
            .map_err(|e| format!("Failed to create shadow dir {}: {e}", shadow_dir.display()))?;

        let head_path = shadow_dir.join("HEAD");
        if !head_path.exists() {
            let output = Command::new("/usr/bin/git")
                .args(["init", "--bare"])
                .arg(shadow_dir)
                .output()
                .map_err(|e| format!("Failed to execute git init --bare: {e}"))?;

            if !output.status.success() {
                return Err(format!(
                    "git init --bare failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ));
            }

            // Configure shadow repo for consistent UTF-8 paths and line endings
            let _ = Command::new("/usr/bin/git")
                .env("GIT_DIR", shadow_dir)
                .args(["config", "core.quotepath", "false"])
                .output();
            let _ = Command::new("/usr/bin/git")
                .env("GIT_DIR", shadow_dir)
                .args(["config", "core.autocrlf", "false"])
                .output();
        }

        Ok(Self {
            workspace_path: workspace_path.to_path_buf(),
            shadow_dir: shadow_dir.to_path_buf(),
        })
    }

    fn git_cmd(&self) -> Command {
        let mut cmd = Command::new("/usr/bin/git");
        cmd.current_dir(&self.workspace_path)
            .env("GIT_DIR", &self.shadow_dir)
            .env("GIT_WORK_TREE", &self.workspace_path)
            .env("GIT_AUTHOR_NAME", "Peck Shadow")
            .env("GIT_AUTHOR_EMAIL", "shadow@peck.local")
            .env("GIT_COMMITTER_NAME", "Peck Shadow")
            .env("GIT_COMMITTER_EMAIL", "shadow@peck.local")
            .args(["-c", "core.quotepath=false", "-c", "core.autocrlf=false"]);
        cmd
    }

    /// Take a snapshot of the current workspace state:
    /// 1. `git add -A` (respecting `.gitignore` for `node_modules/`, `target/`, etc.)
    /// 2. `git write-tree` -> returns `<tree_id>`
    /// 3. `git commit-tree <tree_id> -m <label>` -> anchors the tree in the bare repo
    pub fn create_snapshot(&self, label: &str) -> Result<String, String> {
        // Remove stale index.lock if a previous process crashed mid-operation
        let lock_file = self.shadow_dir.join("index.lock");
        if lock_file.exists() {
            let _ = fs::remove_file(&lock_file);
        }

        let add_out = self
            .git_cmd()
            .args(["add", "-A"])
            .output()
            .map_err(|e| format!("Failed to run shadow git add -A: {e}"))?;

        if !add_out.status.success() {
            return Err(format!(
                "shadow git add -A failed: {}",
                String::from_utf8_lossy(&add_out.stderr)
            ));
        }

        let write_out = self
            .git_cmd()
            .arg("write-tree")
            .output()
            .map_err(|e| format!("Failed to run shadow git write-tree: {e}"))?;

        if !write_out.status.success() {
            return Err(format!(
                "shadow git write-tree failed: {}",
                String::from_utf8_lossy(&write_out.stderr)
            ));
        }

        let tree_id = String::from_utf8_lossy(&write_out.stdout)
            .trim()
            .to_string();
        if tree_id.is_empty() {
            return Err("shadow git write-tree returned empty tree hash".to_string());
        }

        // Anchor the tree in the bare repo with commit-tree
        let _ = self
            .git_cmd()
            .args(["commit-tree", &tree_id, "-m", label])
            .output();

        Ok(tree_id)
    }

    /// Compute the `ChangeSetSummary` (changed files with additions/deletions)
    /// between `tree_before` and `tree_after`.
    pub fn diff_changeset(&self, tree_before: &str, tree_after: &str) -> Result<ChangeSetSummary, String> {
        if tree_before.is_empty() || tree_after.is_empty() || tree_before == tree_after {
            return Ok(ChangeSetSummary::default());
        }

        let status_out = self
            .git_cmd()
            .args([
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-renames",
                "--name-status",
                tree_before,
                tree_after,
            ])
            .output()
            .map_err(|e| format!("Failed to run shadow git diff --name-status: {e}"))?;

        let numstat_out = self
            .git_cmd()
            .args([
                "diff",
                "--no-color",
                "--no-ext-diff",
                "--no-renames",
                "--numstat",
                tree_before,
                tree_after,
            ])
            .output()
            .map_err(|e| format!("Failed to run shadow git diff --numstat: {e}"))?;

        let mut kind_map: HashMap<String, String> = HashMap::new();
        let mut ordered_paths: Vec<String> = Vec::new();

        for line in String::from_utf8_lossy(&status_out.stdout).lines() {
            let mut parts = line.splitn(2, '\t');
            let Some(status_code) = parts.next() else {
                continue;
            };
            let Some(path_raw) = parts.next() else {
                continue;
            };
            let path = path_raw.trim().to_string();
            if path.is_empty() {
                continue;
            }
            let kind = match status_code.trim().chars().next().unwrap_or('M') {
                'A' => "create",
                'D' => "delete",
                _ => "modify",
            };
            kind_map.insert(path.clone(), kind.to_string());
            ordered_paths.push(path);
        }

        let mut stats_map: HashMap<String, (usize, usize)> = HashMap::new();
        for line in String::from_utf8_lossy(&numstat_out.stdout).lines() {
            let parts: Vec<&str> = line.splitn(3, '\t').collect();
            if parts.len() == 3 {
                let adds = parts[0].trim().parse::<usize>().unwrap_or(0);
                let dels = parts[1].trim().parse::<usize>().unwrap_or(0);
                let path = parts[2].trim().to_string();
                stats_map.insert(path, (adds, dels));
            }
        }

        let mut files = Vec::with_capacity(ordered_paths.len());

        for path in ordered_paths {
            let kind = kind_map
                .remove(&path)
                .unwrap_or_else(|| "modify".to_string());
            let (additions, deletions) = stats_map.remove(&path).unwrap_or((0, 0));
            files.push(FileChangeItem { path, kind, additions, deletions });
        }

        Ok(ChangeSetSummary { files })
    }

    /// Read the content of `file_path` at `tree_id` from the bare repo, falling back to the worktree file.
    /// 两棵树之间的整份 Diff（统一格式；审查用）。超过 `limit` 字节截掉，写明截了。
    pub fn diff_text(&self, tree_before: &str, tree_after: &str, limit: usize) -> String {
        let out = self
            .git_cmd()
            .args(["diff", "--no-color", "--no-ext-diff", "--no-renames", tree_before, tree_after])
            .output();
        let text = match out {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
            _ => return String::new(),
        };
        if text.len() <= limit {
            return text;
        }
        let mut end = limit;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}\n…（Diff 太长，后面 {} 字节没放进来；要看就读文件）", &text[..end], text.len() - end)
    }

    pub fn read_file_at_tree(&self, tree_id: &str, file_path: &str) -> Option<String> {
        if !tree_id.is_empty() && !file_path.is_empty() {
            let spec = format!("{tree_id}:{file_path}");
            if let Ok(out) = self.git_cmd().args(["show", &spec]).output() {
                if out.status.success() {
                    return Some(String::from_utf8_lossy(&out.stdout).to_string());
                }
            }
        }

        let disk_path = self.workspace_path.join(file_path);
        fs::read_to_string(disk_path).ok()
    }

    /// 一个文件在两棵树之间的 Diff（统一格式；FILES 用它标出这次加的行）。
    pub fn file_diff(&self, tree_before: &str, tree_after: &str, file_path: &str) -> Result<String, String> {
        let out = self
            .git_cmd()
            .args(["diff", "--no-color", "--no-ext-diff", "--no-renames", tree_before, tree_after, "--", file_path])
            .output()
            .map_err(|e| format!("Failed to execute shadow git diff: {e}"))?;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

/// 「撤销整个任务」要动的一个文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UndoFile {
    pub path: String,
    /// 任务对它做了什么：`create`（撤销时删掉）、`modify`、`delete`（撤销时写回任务前的样子）。
    pub kind: String,
    /// 任务做完以后这个文件又变过（站主自己改的，或后来的任务改的）；撤销一样会回到任务前。
    pub changed_since: bool,
}

/// 一次任务从开始到做完改了哪些文件（测试里核对快照记录用）。
#[cfg(test)]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UndoPreview {
    pub run_id: String,
    pub files: Vec<UndoFile>,
}

impl ShadowRepo {
    /// 把 `files` 里的路径放回 `tree` 的样子：任务新建的删掉，改过、删掉的照任务前写回。
    /// 只动这几个路径；用户仓库的 index、commit、stash 不碰。
    pub fn restore_paths(&self, tree: &str, files: &[UndoFile]) -> Result<(), String> {
        let back: Vec<&str> = files
            .iter()
            .filter(|f| f.kind != "create")
            .map(|f| f.path.as_str())
            .collect();
        for chunk in back.chunks(64) {
            let out = self
                .git_cmd()
                .args(["restore", "--source", tree, "--worktree", "--"])
                .args(chunk)
                .output()
                .map_err(|e| format!("任务前快照读不出来：{e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "没能放回任务前的文件：{}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        }
        for file in files.iter().filter(|f| f.kind == "create") {
            let rel = Path::new(&file.path);
            if rel.is_absolute()
                || rel
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                continue;
            }
            let full = self.workspace_path.join(rel);
            if full.is_file() || full.is_symlink() {
                fs::remove_file(&full).map_err(|e| format!("删不掉 {}：{e}", file.path))?;
            }
            // 任务新建的空文件夹一起收掉；不空的、项目根不动。
            let mut dir = full.parent();
            while let Some(current) = dir {
                if current == self.workspace_path || !current.starts_with(&self.workspace_path) {
                    break;
                }
                if fs::remove_dir(current).is_err() {
                    break;
                }
                dir = current.parent();
            }
        }
        Ok(())
    }
}

/// 测试用：这个任务从开始到做完改了哪些文件，以及做完以后哪些又变过。
/// （站主要的回退 v1.0.2 起走 `harness/extras.rs` 的 `restore_snapshot`；这几个留着核对快照记录放得回去。）
/// 返回的第三项是「撤销前」的快照（撤销之前的样子也留一份在影子仓库里）。
#[cfg(test)]
pub fn undo_preview(record: &RunSnapshotRecord) -> Result<(ShadowRepo, UndoPreview), String> {
    let repo = ShadowRepo::with_shadow_dir(&record.workspace_path, &record.shadow_dir)?;
    let now_tree = repo.create_snapshot(&format!("{}-before-undo", record.run_id))?;
    // 没有步骤收尾过（比如刚开始就被打断）时，拿现在的样子当任务后的样子。
    let after_tree = if record.latest_tree == record.base_tree {
        now_tree.clone()
    } else {
        record.latest_tree.clone()
    };
    let changed = repo.diff_changeset(&record.base_tree, &after_tree)?;
    let since: std::collections::HashSet<String> = repo
        .diff_changeset(&after_tree, &now_tree)?
        .files
        .into_iter()
        .map(|f| f.path)
        .collect();
    let files = changed
        .files
        .into_iter()
        .map(|f| UndoFile {
            changed_since: since.contains(&f.path),
            path: f.path,
            kind: f.kind,
        })
        .collect();
    Ok((
        repo,
        UndoPreview {
            run_id: record.run_id.clone(),
            files,
        },
    ))
}

/// 其他文件夹的撤销预览：每个文件夹（仓库，改过的文件用它自己的相对路径）。
#[cfg(test)]
fn extra_undo(record: &RunSnapshotRecord) -> Result<Vec<(ShadowRepo, ExtraSnapshot, Vec<UndoFile>)>, String> {
    let mut out = Vec::new();
    for extra in &record.extras {
        if !extra.root.is_dir() {
            continue;
        }
        let repo = ShadowRepo::with_shadow_dir(&extra.root, &extra.shadow_dir)?;
        let now_tree = repo.create_snapshot(&format!("{}-extra-before-undo", record.run_id))?;
        let after = if extra.latest_tree == extra.base_tree { now_tree.clone() } else { extra.latest_tree.clone() };
        let changed = repo.diff_changeset(&extra.base_tree, &after)?;
        let since: std::collections::HashSet<String> = repo.diff_changeset(&after, &now_tree)?.files.into_iter().map(|f| f.path).collect();
        let files = changed
            .files
            .into_iter()
            .map(|f| UndoFile { changed_since: since.contains(&f.path), path: f.path, kind: f.kind })
            .collect();
        out.push((repo, extra.clone(), files));
    }
    Ok(out)
}

/// 测试用：任务改过的文件回到任务前快照，别的文件不动。项目的其他文件夹一起。
#[cfg(test)]
pub fn undo_run(record: &RunSnapshotRecord) -> Result<UndoPreview, String> {
    let (repo, mut preview) = undo_preview(record)?;
    repo.restore_paths(&record.base_tree, &preview.files)?;
    for (extra_repo, extra, files) in extra_undo(record)? {
        extra_repo.restore_paths(&extra.base_tree, &files)?;
        preview.files.extend(files.into_iter().map(|f| UndoFile { path: extra.root.join(&f.path).to_string_lossy().into_owned(), ..f }));
    }
    Ok(preview)
}

impl ShadowRepo {
    /// 一个文件在某棵树里的 blob（没有为空）。独立工作区搬回时比对用。
    pub fn blob_id_at(&self, tree_id: &str, file_path: &str) -> Option<String> {
        let out = self.git_cmd().args(["rev-parse", "--verify", "--quiet", &format!("{tree_id}:{file_path}")]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// 工作区里这个文件现在按 git 的口径（照 .gitattributes 的换行、过滤器）算出来的 blob（文件不在为空）。
    pub fn blob_id_of_file(&self, file_path: &str) -> Option<String> {
        if !self.workspace_path.join(file_path).is_file() {
            return None;
        }
        let out = self.git_cmd().args(["hash-object", "--path", file_path, "--"]).arg(self.workspace_path.join(file_path)).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

/// Record the baseline snapshot for a Run when it starts.
pub fn record_run_base(run_id: &str, repo: &ShadowRepo, base_tree: String) {
    if let Ok(mut guard) = snapshot_registry().lock() {
        guard.insert(
            run_id.to_string(),
            RunSnapshotRecord {
                run_id: run_id.to_string(),
                workspace_path: repo.workspace_path.clone(),
                shadow_dir: repo.shadow_dir.clone(),
                base_tree: base_tree.clone(),
                latest_tree: base_tree,
                extras: Vec::new(),
            },
        );
    }
}

/// 任务做完的快照：记成这个任务最新的样子（回退、Diff、独立工作区都按它）。
pub fn record_run_finish(run_id: &str, after_tree: String) {
    if let Ok(mut guard) = snapshot_registry().lock() {
        if let Some(run_rec) = guard.get_mut(run_id) {
            run_rec.latest_tree = after_tree;
        }
    }
}

/// 一个任务改的某个文件的 Diff（任务开始前到做完；还没做完就到现在）。不认识的任务、没改这个文件返回空。
pub fn get_diff_for_run(run_id: &str, file_path: &str) -> Result<String, String> {
    let Some(record) = snapshot_registry().lock().ok().and_then(|g| g.get(run_id).cloned()) else {
        return Ok(String::new());
    };
    // 其他文件夹里的文件（绝对路径）：用那个文件夹自己的影子仓库。
    if let Some((extra, rel)) = record.extras.iter().find_map(|e| e.rel_of(file_path).map(|r| (e.clone(), r))) {
        let repo = ShadowRepo::with_shadow_dir(&extra.root, &extra.shadow_dir)?;
        let after = if extra.base_tree == extra.latest_tree {
            repo.create_snapshot(&format!("{run_id}-extra-live")).unwrap_or_else(|_| extra.latest_tree.clone())
        } else {
            extra.latest_tree.clone()
        };
        return repo.file_diff(&extra.base_tree, &after, &rel);
    }
    let repo = ShadowRepo::with_shadow_dir(&record.workspace_path, &record.shadow_dir)?;
    let after = if record.base_tree == record.latest_tree {
        repo.create_snapshot(&format!("{run_id}-live")).unwrap_or_else(|_| record.latest_tree.clone())
    } else {
        record.latest_tree.clone()
    };
    repo.file_diff(&record.base_tree, &after, file_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shadow_git_snapshot_and_diff_preserves_user_git() {
        let temp_ws = std::env::temp_dir().join(format!("peck-shadow-ws-{}", uuid::Uuid::new_v4()));
        let temp_shadow =
            std::env::temp_dir().join(format!("peck-shadow-bare-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(temp_ws.join("src")).unwrap();

        // 1. Initialize a real user Git repository in temp_ws
        Command::new("/usr/bin/git")
            .args(["init"])
            .arg(&temp_ws)
            .output()
            .unwrap();
        fs::write(temp_ws.join(".gitignore"), "dist/\n").unwrap();
        fs::write(
            temp_ws.join("src/login.ts"),
            "export function login() {\n  return false;\n}\n",
        )
        .unwrap();

        Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&temp_ws)
            .args(["add", "-A"])
            .output()
            .unwrap();
        Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&temp_ws)
            .args([
                "-c",
                "user.name=TestUser",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-m",
                "initial user commit",
            ])
            .output()
            .unwrap();

        let user_head_before = String::from_utf8_lossy(
            &Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&temp_ws)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();

        // 2. Initialize ShadowRepo and take baseline snapshot t0
        let shadow = ShadowRepo::with_shadow_dir(&temp_ws, &temp_shadow).unwrap();
        let t0 = shadow.create_snapshot("run-301-base").unwrap();
        assert_eq!(t0.len(), 40);

        // 3. Simulate Agent writing docs/intent.md, a gitignored build output and modifying src/login.ts
        fs::create_dir_all(temp_ws.join("docs")).unwrap();
        fs::write(
            temp_ws.join("docs/intent.md"),
            "# 登录验证码需求规范\n\n1. 增加图形验证码校验\n",
        )
        .unwrap();
        fs::create_dir_all(temp_ws.join("dist")).unwrap();
        fs::write(temp_ws.join("dist/out.js"), "built\n").unwrap();
        fs::write(
            temp_ws.join("src/login.ts"),
            "export function login(captcha: string) {\n  if (!captcha) return false;\n  return true;\n}\n",
        )
        .unwrap();

        let t1 = shadow.create_snapshot("run-301-step-1-after").unwrap();
        assert_ne!(t0, t1);

        // 4. Verify ChangeSetSummary includes docs/intent.md and src/login.ts, not the gitignored dist/
        let changeset = shadow.diff_changeset(&t0, &t1).unwrap();
        assert_eq!(changeset.files.len(), 2);
        assert!(!changeset.files.iter().any(|f| f.path.starts_with("dist/")));

        let intent_change = changeset
            .files
            .iter()
            .find(|f| f.path == "docs/intent.md")
            .expect("docs/intent.md must be tracked in shadow git");
        assert_eq!(intent_change.kind, "create");
        assert_eq!(intent_change.additions, 3);

        let code_change = changeset
            .files
            .iter()
            .find(|f| f.path == "src/login.ts")
            .expect("src/login.ts modification must be tracked");
        assert_eq!(code_change.kind, "modify");
        assert_eq!(code_change.additions, 3);
        assert_eq!(code_change.deletions, 2);

        // 5. Verify file_diff returns the real unified diff of one file
        let intent_diff = shadow.file_diff(&t0, &t1, "docs/intent.md").unwrap();
        assert!(intent_diff.contains("new file mode"));
        assert!(intent_diff.contains("+# 登录验证码需求规范"));
        assert!(!intent_diff.contains("src/login.ts"));

        let code_diff = shadow.file_diff(&t0, &t1, "src/login.ts").unwrap();
        assert!(code_diff.contains("-export function login() {"));
        assert!(code_diff.contains("+export function login(captcha: string) {"));

        // 6. Verify user's Git HEAD and index were NOT modified by Shadow Git
        let user_head_after = String::from_utf8_lossy(
            &Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&temp_ws)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap()
                .stdout,
        )
        .trim()
        .to_string();
        assert_eq!(user_head_before, user_head_after);

        // Shadow Git never stages anything in the user's index
        let user_status = String::from_utf8_lossy(
            &Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&temp_ws)
                .args(["status", "--porcelain"])
                .output()
                .unwrap()
                .stdout,
        )
        .to_string();
        assert!(user_status.lines().all(|l| !l.starts_with('A') && !l.starts_with("M ")), "{user_status}");

        let _ = fs::remove_dir_all(&temp_ws);
        let _ = fs::remove_dir_all(&temp_shadow);
    }

    #[test]
    fn test_run_snapshot_registry_and_get_diff_for_run() {
        let temp_ws = std::env::temp_dir().join(format!("peck-reg-ws-{}", uuid::Uuid::new_v4()));
        let temp_shadow =
            std::env::temp_dir().join(format!("peck-reg-bare-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_ws).unwrap();

        let shadow = ShadowRepo::with_shadow_dir(&temp_ws, &temp_shadow).unwrap();
        let base_tree = shadow.create_snapshot("base").unwrap();
        record_run_base("run-302", &shadow, base_tree.clone());

        fs::create_dir_all(temp_ws.join("docs")).unwrap();
        fs::write(temp_ws.join("docs/intent.md"), "# Intent 302\nDetails here\n").unwrap();

        let step1_after = shadow.create_snapshot("step1-after").unwrap();
        record_run_finish("run-302", step1_after);

        let diff = get_diff_for_run("run-302", "docs/intent.md").unwrap();
        assert!(diff.contains("+# Intent 302"));
        assert!(get_diff_for_run("run-unknown", "docs/intent.md").unwrap().is_empty());

        let _ = fs::remove_dir_all(&temp_ws);
        let _ = fs::remove_dir_all(&temp_shadow);
    }

    #[test]
    fn undo_puts_task_files_back_and_leaves_everything_else_alone() {
        let ws = std::env::temp_dir().join(format!("peck-undo-ws-{}", uuid::Uuid::new_v4()));
        let shadow = std::env::temp_dir().join(format!("peck-undo-bare-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(ws.join("src")).unwrap();
        Command::new("/usr/bin/git").arg("init").arg(&ws).output().unwrap();
        fs::write(ws.join("src/a.txt"), "任务前\n").unwrap();
        fs::write(ws.join("gone.txt"), "会被任务删掉\n").unwrap();
        fs::write(ws.join("mine.txt"), "站主的文件\n").unwrap();
        let user_index_before = fs::read(ws.join(".git/HEAD")).unwrap();

        let repo = ShadowRepo::with_shadow_dir(&ws, &shadow).unwrap();
        let base = repo.create_snapshot("run-base").unwrap();

        // 任务：改一个、删一个、在新文件夹里建一个。
        fs::write(ws.join("src/a.txt"), "任务改过\n").unwrap();
        fs::remove_file(ws.join("gone.txt")).unwrap();
        fs::create_dir_all(ws.join("src/new/deep")).unwrap();
        fs::write(ws.join("src/new/deep/b.txt"), "任务新建\n").unwrap();
        let after = repo.create_snapshot("run-after").unwrap();

        // 任务做完以后，站主改了自己的文件，又动了任务改过的那个。
        fs::write(ws.join("mine.txt"), "站主后来改的\n").unwrap();
        fs::write(ws.join("src/a.txt"), "任务改过，站主又改\n").unwrap();

        let record = RunSnapshotRecord {
            run_id: "run-undo".into(),
            workspace_path: repo.workspace_path.clone(),
            shadow_dir: repo.shadow_dir.clone(),
            base_tree: base,
            latest_tree: after,
            extras: Vec::new(),
        };
        let (_, preview) = undo_preview(&record).unwrap();
        let by_path: HashMap<&str, &UndoFile> =
            preview.files.iter().map(|f| (f.path.as_str(), f)).collect();
        assert_eq!(preview.files.len(), 3);
        assert_eq!(by_path["src/a.txt"].kind, "modify");
        assert!(by_path["src/a.txt"].changed_since);
        assert_eq!(by_path["gone.txt"].kind, "delete");
        assert!(!by_path["gone.txt"].changed_since);
        assert_eq!(by_path["src/new/deep/b.txt"].kind, "create");

        undo_run(&record).unwrap();
        assert_eq!(fs::read_to_string(ws.join("src/a.txt")).unwrap(), "任务前\n");
        assert_eq!(
            fs::read_to_string(ws.join("gone.txt")).unwrap(),
            "会被任务删掉\n"
        );
        assert!(!ws.join("src/new").exists());
        assert!(ws.join("src").is_dir());
        // 站主后来改的别的文件不动；用户自己的 Git 不动。
        assert_eq!(
            fs::read_to_string(ws.join("mine.txt")).unwrap(),
            "站主后来改的\n"
        );
        assert_eq!(fs::read(ws.join(".git/HEAD")).unwrap(), user_index_before);
        assert!(!ws.join(".git/index").exists());

        let _ = fs::remove_dir_all(&ws);
        let _ = fs::remove_dir_all(&shadow);
    }
}
