use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::events::{EventSink, RunEvent, TaskItem};
use crate::tasks::load_workspace_tasks_summary;

/// Default polling interval for active Step workspace watching.
pub const DEFAULT_WATCH_INTERVAL: Duration = Duration::from_millis(80);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    pub len: u64,
    pub modified_ns: u128,
}

/// Parsed `.gitignore` rule matcher that respects workspace ignore patterns
/// while **always force-including `.peck/`**.
#[derive(Debug, Clone, Default)]
pub struct GitignoreRules {
    patterns: Vec<IgnorePattern>,
}

#[derive(Debug, Clone)]
struct IgnorePattern {
    raw: String,
    dir_only: bool,
    anchored: bool,
}

impl GitignoreRules {
    /// Load `.gitignore` from `workspace_path` if present.
    pub fn from_workspace(workspace_path: &Path) -> Self {
        let gitignore_file = workspace_path.join(".gitignore");
        let Ok(raw) = fs::read_to_string(gitignore_file) else {
            return Self::default();
        };
        Self::from_gitignore_str(&raw)
    }

    pub fn from_gitignore_str(raw: &str) -> Self {
        let mut patterns = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
                continue;
            }
            let dir_only = trimmed.ends_with('/');
            let without_trailing = trimmed.trim_end_matches('/');
            let anchored = without_trailing.starts_with('/');
            let clean = without_trailing.trim_start_matches('/').to_string();

            // Never ignore `.peck` in the workspace watcher
            if clean == ".peck" || clean.starts_with(".peck/") {
                continue;
            }
            if !clean.is_empty() {
                patterns.push(IgnorePattern {
                    raw: clean,
                    dir_only,
                    anchored,
                });
            }
        }
        Self { patterns }
    }

    /// Returns `true` if `rel_path` (using `/` separators) should be ignored.
    /// Rule: `.git` is always ignored; `.peck` and `.peck/**` are NEVER ignored.
    pub fn is_ignored(&self, rel_path: &str, is_dir: bool) -> bool {
        let normalized = rel_path.trim_matches('/');
        if normalized.is_empty() {
            return false;
        }

        // Force-include `.peck` and anything inside `.peck/`
        if normalized == ".peck" || normalized.starts_with(".peck/") {
            return false;
        }

        let file_name = normalized.rsplit('/').next().unwrap_or(normalized);

        // Built-in VCS and heavy system directories that must always be skipped
        if file_name == ".git" || file_name == ".DS_Store" {
            return true;
        }

        let segments: Vec<&str> = normalized.split('/').collect();

        for pat in &self.patterns {
            if pat.anchored || pat.raw.contains('/') {
                if (!pat.dir_only || is_dir) && wildcard_match(&pat.raw, normalized) {
                    return true;
                }
                if normalized.starts_with(&format!("{}/", pat.raw)) {
                    return true;
                }
            } else {
                // Match against any parent directory segment (e.g. `node_modules/` matching `node_modules/react/index.js`)
                if segments.len() > 1
                    && segments[..segments.len() - 1]
                        .iter()
                        .any(|seg| wildcard_match(&pat.raw, seg))
                {
                    return true;
                }
                if (!pat.dir_only || is_dir)
                    && (wildcard_match(&pat.raw, file_name) || wildcard_match(&pat.raw, normalized))
                {
                    return true;
                }
            }
        }

        false
    }
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern == text || pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return pattern == text;
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.is_empty() {
        return true;
    }

    let mut pos = 0;
    for (idx, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if idx == 0 {
            if !text.starts_with(part) {
                return false;
            }
            pos = part.len();
        } else if idx == parts.len() - 1 {
            if !text[pos..].ends_with(part) {
                return false;
            }
            pos = text.len();
        } else if let Some(found) = text[pos..].find(part) {
            pos += found + part.len();
        } else {
            return false;
        }
    }
    true
}

/// Scan `workspace_path` and collect `FileStamp`s for all non-ignored files (force-including `.peck/`).
pub fn scan_workspace_files(
    workspace_path: &Path,
    rules: &GitignoreRules,
) -> BTreeMap<String, FileStamp> {
    let mut map = BTreeMap::new();
    walk_dir_recursive(workspace_path, workspace_path, rules, &mut map);
    map
}

fn walk_dir_recursive(
    root: &Path,
    current: &Path,
    rules: &GitignoreRules,
    out: &mut BTreeMap<String, FileStamp>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel_str = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");

        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }

        let is_dir = file_type.is_dir();
        if rules.is_ignored(&rel_str, is_dir) {
            continue;
        }

        if is_dir {
            walk_dir_recursive(root, &path, rules, out);
        } else if file_type.is_file() {
            if let Ok(meta) = entry.metadata() {
                let modified_ns = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                out.insert(
                    rel_str,
                    FileStamp {
                        len: meta.len(),
                        modified_ns,
                    },
                );
            }
        }
    }
}

struct WatcherLoopState {
    workspace_path: PathBuf,
    run_id: String,
    step_id: String,
    sink: EventSink,
    rules: GitignoreRules,
    baseline_files: BTreeMap<String, FileStamp>,
    current_files: BTreeMap<String, FileStamp>,
    last_tasks: Vec<TaskItem>,
    touched_paths: HashSet<String>,
}

impl WatcherLoopState {
    fn new(workspace_path: PathBuf, run_id: String, step_id: String, sink: EventSink) -> Self {
        let rules = GitignoreRules::from_workspace(&workspace_path);
        let initial = scan_workspace_files(&workspace_path, &rules);
        let initial_tasks = load_workspace_tasks_summary(&workspace_path)
            .map(|s| s.items)
            .unwrap_or_default();

        Self {
            workspace_path,
            run_id,
            step_id,
            sink,
            rules,
            baseline_files: initial.clone(),
            current_files: initial,
            last_tasks: initial_tasks,
            touched_paths: HashSet::new(),
        }
    }

    fn poll_once(&mut self) {
        let next_files = scan_workspace_files(&self.workspace_path, &self.rules);

        // Check created or modified files
        for (path, next_stamp) in &next_files {
            let changed = match self.current_files.get(path) {
                None => true,
                Some(prev_stamp) => prev_stamp != next_stamp,
            };

            if changed {
                let kind = if self.baseline_files.contains_key(path) {
                    "modify"
                } else {
                    "create"
                };
                self.touched_paths.insert(path.clone());
                self.sink.emit(RunEvent::FileTouched {
                    run_id: self.run_id.clone(),
                    step_id: self.step_id.clone(),
                    path: path.clone(),
                    kind: kind.to_string(),
                });
            }
        }

        // Check deleted files
        for prev_path in self.current_files.keys() {
            if !next_files.contains_key(prev_path) {
                self.touched_paths.insert(prev_path.clone());
                self.sink.emit(RunEvent::FileTouched {
                    run_id: self.run_id.clone(),
                    step_id: self.step_id.clone(),
                    path: prev_path.clone(),
                    kind: "delete".to_string(),
                });
            }
        }

        self.current_files = next_files;

        // Check `.peck/tasks.md` checkbox progress updates
        if let Some(summary) = load_workspace_tasks_summary(&self.workspace_path) {
            if summary.items != self.last_tasks {
                self.last_tasks = summary.items.clone();
                self.sink.emit(RunEvent::TasksUpdated {
                    run_id: self.run_id.clone(),
                    step_id: self.step_id.clone(),
                    total: summary.total,
                    completed: summary.completed,
                    items: summary.items,
                });
            }
        }
    }
}

/// Active workspace watcher running alongside a Step execution.
pub struct WorkspaceWatcher {
    stop_tx: Option<oneshot::Sender<()>>,
    join_handle: JoinHandle<HashSet<String>>,
}

impl WorkspaceWatcher {
    /// Start watching `workspace_path` for a running Step and streaming `FileTouched` / `TasksUpdated`.
    pub fn start(
        workspace_path: PathBuf,
        run_id: String,
        step_id: String,
        sink: EventSink,
        poll_interval: Duration,
    ) -> Self {
        let mut state = WatcherLoopState::new(workspace_path, run_id, step_id, sink);
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();

        let join_handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(poll_interval);
            // Skip immediate first tick since baseline was just captured
            ticker.tick().await;

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        state.poll_once();
                    }
                    _ = &mut stop_rx => {
                        // Final flush pass after child process finishes
                        state.poll_once();
                        break;
                    }
                }
            }

            state.touched_paths
        });

        Self {
            stop_tx: Some(stop_tx),
            join_handle,
        }
    }

    /// Stop the watcher loop, perform one final flush scan, and return all touched relative paths.
    pub async fn stop_and_flush(mut self) -> HashSet<String> {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        self.join_handle.await.unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gitignore_rules_respects_gitignore_but_forces_peck_inclusion() {
        let rules = GitignoreRules::from_gitignore_str(
            r#"
            # Dependencies & build
            node_modules/
            dist/
            *.log
            .peck/
            "#,
        );

        assert!(rules.is_ignored("node_modules", true));
        assert!(rules.is_ignored("node_modules/react/index.js", false));
        assert!(rules.is_ignored("dist", true));
        assert!(rules.is_ignored("npm-debug.log", false));
        assert!(rules.is_ignored(".git", true));

        // `.peck/` MUST NEVER be ignored even though `.peck/` is in `.gitignore`
        assert!(!rules.is_ignored(".peck", true));
        assert!(!rules.is_ignored(".peck/tasks.md", false));
        assert!(!rules.is_ignored(".peck/handoff/run-105/intent.md", false));
        assert!(!rules.is_ignored("src/App.tsx", false));
    }

    #[tokio::test]
    async fn test_watcher_emits_file_touched_and_tasks_updated_realtime() {
        let temp_ws =
            std::env::temp_dir().join(format!("peck-watch-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(temp_ws.join("src")).unwrap();
        fs::write(temp_ws.join(".gitignore"), "node_modules/\n.peck/\n").unwrap();
        fs::write(temp_ws.join("src/existing.ts"), "export const v = 1;\n").unwrap();

        let (sink, events_buf) = EventSink::new_memory();

        let watcher = WorkspaceWatcher::start(
            temp_ws.clone(),
            "run-310".to_string(),
            "step-1".to_string(),
            sink,
            Duration::from_millis(25),
        );

        // Ignored directory write should NOT emit FileTouched
        fs::create_dir_all(temp_ws.join("node_modules/pkg")).unwrap();
        fs::write(temp_ws.join("node_modules/pkg/index.js"), "ignored").unwrap();

        // Write into .peck/ and modify src/existing.ts
        fs::create_dir_all(temp_ws.join(".peck/handoff/run-310")).unwrap();
        fs::write(
            temp_ws.join(".peck/handoff/run-310/intent.md"),
            "# Intent\n",
        )
        .unwrap();
        fs::write(
            temp_ws.join(".peck/tasks.md"),
            "- [x] Task A\n- [ ] Task B\n",
        )
        .unwrap();
        fs::write(temp_ws.join("src/existing.ts"), "export const v = 2;\n").unwrap();

        tokio::time::sleep(Duration::from_millis(70)).await;
        let touched = watcher.stop_and_flush().await;

        assert!(touched.contains(".peck/handoff/run-310/intent.md"));
        assert!(touched.contains(".peck/tasks.md"));
        assert!(touched.contains("src/existing.ts"));
        assert!(!touched.contains("node_modules/pkg/index.js"));

        let events = events_buf.lock().unwrap();
        let has_intent_create = events.iter().any(|e| {
            matches!(
                e,
                RunEvent::FileTouched { path, kind, .. }
                if path == ".peck/handoff/run-310/intent.md" && kind == "create"
            )
        });
        let has_existing_modify = events.iter().any(|e| {
            matches!(
                e,
                RunEvent::FileTouched { path, kind, .. }
                if path == "src/existing.ts" && kind == "modify"
            )
        });
        let has_tasks_updated = events.iter().any(|e| {
            matches!(
                e,
                RunEvent::TasksUpdated {
                    total: 2,
                    completed: 1,
                    ..
                }
            )
        });

        assert!(has_intent_create);
        assert!(has_existing_modify);
        assert!(has_tasks_updated);

        let _ = fs::remove_dir_all(&temp_ws);
    }
}
