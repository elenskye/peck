use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use crate::events::{ChangeSetSummary, FileChangeItem};
use crate::mode::app_data_dir;
use crate::workspace::{normalize_workspace_path, workspace_hash};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiffResult {
    pub run_id: String,
    pub step_id: Option<String>,
    pub file_path: String,
    pub kind: String,
    pub language: String,
    pub additions: usize,
    pub deletions: usize,
    pub unified_diff: String,
    pub content_after: Option<String>,
}

/// Shadow Git bare repository controller.
/// Uses `GIT_DIR=<shadow_dir>` and `GIT_WORK_TREE=<workspace_path>` so that user repository
/// `.git/index`, commit history, and stashes are never touched.
#[derive(Debug, Clone)]
pub struct ShadowRepo {
    pub workspace_path: PathBuf,
    pub shadow_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepSnapshotEntry {
    pub before_tree: String,
    pub after_tree: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSnapshotRecord {
    pub run_id: String,
    pub workspace_path: PathBuf,
    pub shadow_dir: PathBuf,
    pub base_tree: String,
    pub latest_tree: String,
    pub steps: HashMap<String, StepSnapshotEntry>,
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
    /// 2. `git add -f -A -- .peck` (if `.peck/` exists, force-tracking handoff & tasks artifacts)
    /// 3. `git write-tree` -> returns `<tree_id>`
    /// 4. `git commit-tree <tree_id> -m <label>` -> anchors the tree in the bare repo
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

        if self.workspace_path.join(".peck").exists() {
            let _ = self
                .git_cmd()
                .args(["add", "-f", "-A", "--", ".peck"])
                .output();
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

    /// Compute the `ChangeSetSummary` (changed files list + additions/deletions counts)
    /// between `tree_before` and `tree_after`.
    pub fn diff_changeset(
        &self,
        tree_before: &str,
        tree_after: &str,
        step_id: &str,
    ) -> Result<ChangeSetSummary, String> {
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
        let mut total_additions = 0;
        let mut total_deletions = 0;

        for path in ordered_paths {
            let kind = kind_map
                .remove(&path)
                .unwrap_or_else(|| "modify".to_string());
            let (additions, deletions) = stats_map.remove(&path).unwrap_or((0, 0));
            total_additions += additions;
            total_deletions += deletions;
            files.push(FileChangeItem {
                path,
                kind,
                additions,
                deletions,
                step_id: step_id.to_string(),
            });
        }

        Ok(ChangeSetSummary {
            files,
            total_additions,
            total_deletions,
        })
    }

    /// Read the content of `file_path` at `tree_id` from the bare repo, falling back to the worktree file.
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

    /// Compute the `DiffResult` between `tree_before` and `tree_after` for a specific `file_path`
    /// (or the first changed file / whole tree if `file_path` is `None`).
    pub fn diff_file(
        &self,
        run_id: &str,
        step_id: Option<&str>,
        tree_before: &str,
        tree_after: &str,
        file_path: Option<&str>,
    ) -> Result<DiffResult, String> {
        let changeset = self
            .diff_changeset(tree_before, tree_after, step_id.unwrap_or(""))
            .unwrap_or_default();

        let resolved_path = match file_path.map(str::trim).filter(|s| !s.is_empty()) {
            Some(p) => p.to_string(),
            None => changeset
                .files
                .first()
                .map(|f| f.path.clone())
                .unwrap_or_default(),
        };

        let mut diff_cmd = self.git_cmd();
        diff_cmd.args([
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-renames",
            tree_before,
            tree_after,
        ]);
        if !resolved_path.is_empty() {
            diff_cmd.args(["--", &resolved_path]);
        }

        let diff_out = diff_cmd
            .output()
            .map_err(|e| format!("Failed to execute shadow git diff: {e}"))?;

        let mut unified_diff = String::from_utf8_lossy(&diff_out.stdout).to_string();

        let matched_item = changeset.files.iter().find(|f| f.path == resolved_path);
        let mut kind = matched_item.map(|f| f.kind.clone()).unwrap_or_else(|| {
            if unified_diff.contains("new file mode") {
                "create".to_string()
            } else if unified_diff.contains("deleted file mode") {
                "delete".to_string()
            } else {
                "modify".to_string()
            }
        });

        let content_after = if kind == "delete" || resolved_path.is_empty() {
            None
        } else {
            self.read_file_at_tree(tree_after, &resolved_path)
        };

        // If the requested file wasn't in `tree_before..tree_after` (e.g. viewing a file from Step 1
        // while Step 2 is selected, or inspecting an untracked/existing file), synthesize a clean diff
        // if the file exists on disk so the preview is never blank.
        if unified_diff.trim().is_empty() {
            if let Some(ref content) = content_after {
                kind = "create".to_string();
                unified_diff = synthesize_creation_diff(&resolved_path, content);
            }
        }

        let (additions, deletions) = match matched_item {
            Some(item) if item.additions > 0 || item.deletions > 0 => {
                (item.additions, item.deletions)
            }
            _ => count_diff_lines(&unified_diff),
        };

        let language = infer_language(&resolved_path).to_string();

        Ok(DiffResult {
            run_id: run_id.to_string(),
            step_id: step_id.map(|s| s.to_string()),
            file_path: resolved_path,
            kind,
            language,
            additions,
            deletions,
            unified_diff,
            content_after,
        })
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

/// 撤销前先看一眼：会动哪些文件。
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

/// 这个任务从开始到做完改了哪些文件，以及做完以后哪些又变过。
/// 返回的第三项是「撤销前」的快照（撤销之前的样子也留一份在影子仓库里）。
pub fn undo_preview(record: &RunSnapshotRecord) -> Result<(ShadowRepo, UndoPreview), String> {
    let repo = ShadowRepo::with_shadow_dir(&record.workspace_path, &record.shadow_dir)?;
    let now_tree = repo.create_snapshot(&format!("{}-before-undo", record.run_id))?;
    // 没有步骤收尾过（比如刚开始就被打断）时，拿现在的样子当任务后的样子。
    let after_tree = if record.latest_tree == record.base_tree {
        now_tree.clone()
    } else {
        record.latest_tree.clone()
    };
    let changed = repo.diff_changeset(&record.base_tree, &after_tree, "")?;
    let since: std::collections::HashSet<String> = repo
        .diff_changeset(&after_tree, &now_tree, "")?
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
fn extra_undo(record: &RunSnapshotRecord) -> Result<Vec<(ShadowRepo, ExtraSnapshot, Vec<UndoFile>)>, String> {
    let mut out = Vec::new();
    for extra in &record.extras {
        if !extra.root.is_dir() {
            continue;
        }
        let repo = ShadowRepo::with_shadow_dir(&extra.root, &extra.shadow_dir)?;
        let now_tree = repo.create_snapshot(&format!("{}-extra-before-undo", record.run_id))?;
        let after = if extra.latest_tree == extra.base_tree { now_tree.clone() } else { extra.latest_tree.clone() };
        let changed = repo.diff_changeset(&extra.base_tree, &after, "")?;
        let since: std::collections::HashSet<String> = repo.diff_changeset(&after, &now_tree, "")?.files.into_iter().map(|f| f.path).collect();
        let files = changed
            .files
            .into_iter()
            .map(|f| UndoFile { changed_since: since.contains(&f.path), path: f.path, kind: f.kind })
            .collect();
        out.push((repo, extra.clone(), files));
    }
    Ok(out)
}

/// 撤销前的预览（主文件夹 + 其他文件夹；其他文件夹的写成绝对路径）。
pub fn undo_preview_all(record: &RunSnapshotRecord) -> Result<UndoPreview, String> {
    let (_, mut preview) = undo_preview(record)?;
    for (_, extra, files) in extra_undo(record)? {
        preview.files.extend(files.into_iter().map(|f| UndoFile { path: extra.root.join(&f.path).to_string_lossy().into_owned(), ..f }));
    }
    Ok(preview)
}

/// 撤销整个任务：任务改过的文件回到任务前快照，别的文件不动。项目的其他文件夹一起。
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

fn synthesize_creation_diff(file_path: &str, content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let count = lines.len();
    let mut out = format!(
        "diff --git a/{file_path} b/{file_path}\nnew file mode 100644\n--- /dev/null\n+++ b/{file_path}\n@@ -0,0 +1,{count} @@\n"
    );
    for line in lines {
        out.push('+');
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn count_diff_lines(unified_diff: &str) -> (usize, usize) {
    let mut adds = 0;
    let mut dels = 0;
    for line in unified_diff.lines() {
        if line.starts_with("+++ ") || line.starts_with("--- ") {
            continue;
        }
        if line.starts_with('+') {
            adds += 1;
        } else if line.starts_with('-') {
            dels += 1;
        }
    }
    (adds, dels)
}

pub fn infer_language(file_path: &str) -> &'static str {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "md" | "markdown" => "markdown",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" | "mjs" | "cjs" => "javascript",
        "rs" => "rust",
        "json" => "json",
        "toml" => "toml",
        "css" => "css",
        "html" => "html",
        "py" => "python",
        "sh" | "zsh" | "bash" => "shell",
        "yaml" | "yml" => "yaml",
        _ => "plaintext",
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
                steps: HashMap::new(),
                extras: Vec::new(),
            },
        );
    }
}

/// Record the `before_tree` snapshot for a specific Step in a Run.
pub fn record_step_start(run_id: &str, step_id: &str, repo: &ShadowRepo, before_tree: String) {
    if let Ok(mut guard) = snapshot_registry().lock() {
        let entry = guard
            .entry(run_id.to_string())
            .or_insert_with(|| RunSnapshotRecord {
                run_id: run_id.to_string(),
                workspace_path: repo.workspace_path.clone(),
                shadow_dir: repo.shadow_dir.clone(),
                base_tree: before_tree.clone(),
                latest_tree: before_tree.clone(),
                steps: HashMap::new(),
                extras: Vec::new(),
            });
        entry.steps.insert(
            step_id.to_string(),
            StepSnapshotEntry {
                before_tree,
                after_tree: None,
            },
        );
    }
}

/// Record the `after_tree` snapshot for a specific Step in a Run and advance `latest_tree`.
pub fn record_step_finish(run_id: &str, step_id: &str, after_tree: String) {
    if let Ok(mut guard) = snapshot_registry().lock() {
        if let Some(run_rec) = guard.get_mut(run_id) {
            run_rec.latest_tree = after_tree.clone();
            if let Some(step_rec) = run_rec.steps.get_mut(step_id) {
                step_rec.after_tree = Some(after_tree);
            }
        }
    }
}

/// Retrieve the `DiffResult` for a Run (and optional Step / file path) using the Shadow Git repository.
pub fn get_diff_for_run(
    run_id: &str,
    step_id: Option<&str>,
    file_path: Option<&str>,
    fallback_workspace: Option<&Path>,
) -> Result<DiffResult, String> {
    let record_opt = snapshot_registry()
        .lock()
        .ok()
        .and_then(|g| g.get(run_id).cloned());

    if let Some(record) = record_opt {
        // 其他文件夹里的文件（绝对路径）：用那个文件夹自己的影子仓库。
        if let Some((extra, rel)) = file_path.and_then(|p| record.extras.iter().find_map(|e| e.rel_of(p).map(|r| (e.clone(), r)))) {
            let repo = ShadowRepo::with_shadow_dir(&extra.root, &extra.shadow_dir)?;
            let after = if extra.base_tree == extra.latest_tree {
                repo.create_snapshot(&format!("{run_id}-extra-live")).unwrap_or_else(|_| extra.latest_tree.clone())
            } else {
                extra.latest_tree.clone()
            };
            let mut diff = repo.diff_file(run_id, step_id, &extra.base_tree, &after, Some(&rel))?;
            diff.file_path = file_path.unwrap_or_default().to_string();
            return Ok(diff);
        }
        let repo = ShadowRepo::with_shadow_dir(&record.workspace_path, &record.shadow_dir)?;

        let (tree_before, tree_after) = if let Some(sid) = step_id {
            if let Some(step_entry) = record.steps.get(sid) {
                let after = match &step_entry.after_tree {
                    Some(t) => t.clone(),
                    None => repo
                        .create_snapshot(&format!("{sid}-live"))
                        .unwrap_or_else(|_| record.latest_tree.clone()),
                };
                (step_entry.before_tree.clone(), after)
            } else {
                (record.base_tree.clone(), record.latest_tree.clone())
            }
        } else {
            let after = if record.base_tree == record.latest_tree {
                repo.create_snapshot(&format!("{run_id}-live"))
                    .unwrap_or_else(|_| record.latest_tree.clone())
            } else {
                record.latest_tree.clone()
            };
            (record.base_tree.clone(), after)
        };

        return repo.diff_file(run_id, step_id, &tree_before, &tree_after, file_path);
    }

    // Fallback if run_id wasn't found in the registry but a workspace is available
    if let Some(ws_path) = fallback_workspace {
        if let Some(rel_path) = file_path.filter(|p| !p.is_empty()) {
            let full_path = ws_path.join(rel_path);
            if let Ok(content) = fs::read_to_string(&full_path) {
                let unified_diff = synthesize_creation_diff(rel_path, &content);
                let (additions, deletions) = count_diff_lines(&unified_diff);
                return Ok(DiffResult {
                    run_id: run_id.to_string(),
                    step_id: step_id.map(|s| s.to_string()),
                    file_path: rel_path.to_string(),
                    kind: "create".to_string(),
                    language: infer_language(rel_path).to_string(),
                    additions,
                    deletions,
                    unified_diff,
                    content_after: Some(content),
                });
            }
        }
    }

    Ok(DiffResult {
        run_id: run_id.to_string(),
        step_id: step_id.map(|s| s.to_string()),
        file_path: file_path.unwrap_or_default().to_string(),
        kind: "modify".to_string(),
        language: infer_language(file_path.unwrap_or_default()).to_string(),
        additions: 0,
        deletions: 0,
        unified_diff: String::new(),
        content_after: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::ensure_peck_in_gitignore;

    #[test]
    fn test_shadow_git_snapshot_and_diff_preserves_user_git_and_tracks_peck() {
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
        ensure_peck_in_gitignore(&temp_ws).unwrap();
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

        // 3. Simulate Agent writing .peck/handoff/run-301/intent.md and modifying src/login.ts
        let handoff_dir = temp_ws.join(".peck/handoff/run-301");
        fs::create_dir_all(&handoff_dir).unwrap();
        fs::write(
            handoff_dir.join("intent.md"),
            "# 登录验证码需求规范\n\n1. 增加图形验证码校验\n",
        )
        .unwrap();
        fs::write(
            temp_ws.join("src/login.ts"),
            "export function login(captcha: string) {\n  if (!captcha) return false;\n  return true;\n}\n",
        )
        .unwrap();

        let t1 = shadow.create_snapshot("run-301-step-1-after").unwrap();
        assert_ne!(t0, t1);

        // 4. Verify ChangeSetSummary includes BOTH .peck/handoff/run-301/intent.md and src/login.ts
        let changeset = shadow.diff_changeset(&t0, &t1, "step-1").unwrap();
        assert_eq!(changeset.files.len(), 2);

        let intent_change = changeset
            .files
            .iter()
            .find(|f| f.path == ".peck/handoff/run-301/intent.md")
            .expect("intent.md must be tracked in shadow git despite .gitignore");
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

        // 5. Verify diff_file returns real Unified Diff and content_after
        let intent_diff = shadow
            .diff_file(
                "run-301",
                Some("step-1"),
                &t0,
                &t1,
                Some(".peck/handoff/run-301/intent.md"),
            )
            .unwrap();
        assert_eq!(intent_diff.kind, "create");
        assert_eq!(intent_diff.language, "markdown");
        assert!(intent_diff.unified_diff.contains("+# 登录验证码需求规范"));
        assert!(intent_diff
            .content_after
            .as_deref()
            .unwrap_or("")
            .contains("增加图形验证码校验"));

        let code_diff = shadow
            .diff_file("run-301", Some("step-1"), &t0, &t1, Some("src/login.ts"))
            .unwrap();
        assert_eq!(code_diff.kind, "modify");
        assert_eq!(code_diff.language, "typescript");
        assert!(code_diff
            .unified_diff
            .contains("-export function login() {"));
        assert!(code_diff
            .unified_diff
            .contains("+export function login(captcha: string) {"));

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

        // User's git status should NOT show .peck/ as untracked or staged
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
        assert!(!user_status.contains(".peck"));

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
        record_step_start("run-302", "run-302-step-intent", &shadow, base_tree);

        let handoff_dir = temp_ws.join(".peck/handoff/run-302");
        fs::create_dir_all(&handoff_dir).unwrap();
        fs::write(
            handoff_dir.join("intent.md"),
            "# Intent 302\nDetails here\n",
        )
        .unwrap();

        let step1_after = shadow.create_snapshot("step1-after").unwrap();
        record_step_finish("run-302", "run-302-step-intent", step1_after);

        let diff = get_diff_for_run(
            "run-302",
            Some("run-302-step-intent"),
            Some(".peck/handoff/run-302/intent.md"),
            None,
        )
        .unwrap();

        assert_eq!(diff.run_id, "run-302");
        assert_eq!(diff.kind, "create");
        assert_eq!(diff.additions, 2);
        assert!(diff.unified_diff.contains("+# Intent 302"));

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
            steps: HashMap::new(),
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

// ───────────────────────── 按块拒绝（v0.7 Phase 10） ─────────────────────────

/// Diff 里的一块：任务前的那几行、任务后的那几行（都带上下文），和它在任务后的文件里从第几行起。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old: Vec<String>,
    pub new: Vec<String>,
    pub new_start: usize,
}

/// 把统一格式的 Diff 拆成一块一块。
pub fn parse_hunks(unified: &str) -> Vec<Hunk> {
    let mut out: Vec<Hunk> = Vec::new();
    for line in unified.lines() {
        if let Some(rest) = line.strip_prefix("@@") {
            let new_start = rest
                .split_whitespace()
                .find(|w| w.starts_with('+'))
                .and_then(|w| w.trim_start_matches('+').split(',').next())
                .and_then(|n| n.parse().ok())
                .unwrap_or(1);
            out.push(Hunk { old: vec![], new: vec![], new_start });
            continue;
        }
        let Some(h) = out.last_mut() else { continue };
        if line.starts_with("\\ No newline") {
            continue;
        }
        match line.chars().next() {
            Some('+') => h.new.push(line[1..].to_string()),
            Some('-') => h.old.push(line[1..].to_string()),
            Some(' ') => {
                h.old.push(line[1..].to_string());
                h.new.push(line[1..].to_string());
            }
            None => {
                h.old.push(String::new());
                h.new.push(String::new());
            }
            _ => {}
        }
    }
    out
}

/// 拒绝一块：在现在的文件里找到这一块任务后的样子，换回任务前的。`current` 为空是文件现在不在。
/// 返回新的内容（`None` 是文件该删掉：任务新建的文件整块拒掉）。找不到或者有好几处对得上又定不下来时报错。
pub fn revert_hunk(current: Option<&str>, unified: &str, index: usize, created: bool) -> Result<Option<String>, String> {
    let hunks = parse_hunks(unified);
    let hunk = hunks.get(index).ok_or("没有这一块。")?;
    let text = current.unwrap_or("");
    let trailing = text.is_empty() || text.ends_with('\n');
    // 换行照文件原来的（CRLF 的文件拒一块不变成整份 LF）；比对时不看行尾的 \r。
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let bare = |v: &[String]| v.iter().map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect::<Vec<String>>();
    let (old_lines, new_lines) = (bare(&hunk.old), bare(&hunk.new));
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    // 任务删掉了整个文件（`+0,0`）：现在又有内容（站主自己建了一个），不往里插旧的。
    if hunk.new.is_empty() && hunk.new_start == 0 && !text.is_empty() {
        return Err("这个文件后来又有了，和任务做完时对不上，没动。".into());
    }
    let at = if hunk.new.is_empty() {
        // 任务只删了行（这一块没有任务后的行）：照行号放回去。
        Some(hunk.new_start.min(lines.len()))
    } else {
        let matches: Vec<usize> = (0..lines.len().saturating_sub(new_lines.len() - 1))
            .filter(|&i| lines[i..i + new_lines.len()] == new_lines[..])
            .collect();
        let expected = hunk.new_start.saturating_sub(1);
        match matches.as_slice() {
            [] => None,
            [one] => Some(*one),
            many => many.iter().copied().find(|i| *i == expected).or(None),
        }
    };
    let Some(at) = at else {
        return Err("这一块后来又改过了，和任务做完时对不上，没动。".into());
    };
    let end = (at + new_lines.len()).min(lines.len());
    lines.splice(at..end, old_lines);
    if lines.is_empty() && created {
        return Ok(None);
    }
    let mut out = lines.join(eol);
    if trailing && !out.is_empty() {
        out.push_str(eol);
    }
    Ok(Some(out))
}

#[cfg(test)]
mod hunk_tests {
    use super::*;

    const DIFF: &str = "--- a/f\n+++ b/f\n@@ -1,4 +1,4 @@\n a\n-b\n+B\n c\n d\n@@ -8,3 +8,4 @@\n h\n i\n+NEW\n j\n";

    #[test]
    fn hunks_parse_with_context() {
        let h = parse_hunks(DIFF);
        assert_eq!(h.len(), 2);
        assert_eq!((h[0].old.clone(), h[0].new.clone(), h[0].new_start), (vec!["a".into(), "b".into(), "c".into(), "d".into()], vec!["a".into(), "B".into(), "c".into(), "d".into()], 1));
        assert_eq!(h[1].new, vec!["h".to_string(), "i".into(), "NEW".into(), "j".into()]);
    }

    #[test]
    fn rejecting_one_hunk_leaves_the_others() {
        let after = "a\nB\nc\nd\ne\nf\ng\nh\ni\nNEW\nj\n";
        let back = revert_hunk(Some(after), DIFF, 1, false).unwrap().unwrap();
        assert_eq!(back, "a\nB\nc\nd\ne\nf\ng\nh\ni\nj\n");
        let back = revert_hunk(Some(&back), DIFF, 0, false).unwrap().unwrap();
        assert_eq!(back, "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n");
        // 后来又改过：对不上就不动
        assert!(revert_hunk(Some("a\nX\nc\nd\n"), DIFF, 0, false).unwrap_err().contains("对不上"));
        assert!(revert_hunk(Some(after), DIFF, 5, false).is_err());
    }

    #[test]
    fn rejecting_keeps_crlf_and_refuses_a_recreated_file() {
        let after = "a\r\nB\r\nc\r\nd\r\ne\r\n";
        let back = revert_hunk(Some(after), DIFF, 0, false).unwrap().unwrap();
        assert_eq!(back, "a\r\nb\r\nc\r\nd\r\ne\r\n");
        // 任务删了整个文件，站主又建了一个：不往新文件里插旧内容
        let gone = "--- a/f\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-x\n-y\n";
        assert!(revert_hunk(Some("new\n"), gone, 0, false).unwrap_err().contains("对不上"));
        assert_eq!(revert_hunk(None, gone, 0, false).unwrap().unwrap(), "x\ny\n");
    }

    #[test]
    fn rejecting_a_new_file_deletes_it_and_a_deleted_file_comes_back() {
        let created = "--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+x\n+y\n";
        assert_eq!(revert_hunk(Some("x\ny\n"), created, 0, true).unwrap(), None);
        let deleted = "--- a/n\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-x\n-y\n";
        assert_eq!(revert_hunk(None, deleted, 0, false).unwrap(), Some("x\ny\n".into()));
    }
}
