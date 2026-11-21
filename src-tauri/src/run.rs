//! 在跑的任务的登记（v0.8 删了旧的按清单一步步起 CLI 的派发：接力、假 Agent 都并进了新的发任务）。
use std::path::PathBuf;

use crate::process::ProcessCancelHandle;

/// 在跑的任务（`AppState.active_runs`）：停止开关、在哪个项目、会写哪些文件夹。
#[derive(Clone)]
pub struct ActiveRunContext {
    pub workspace_path: PathBuf,
    pub cancel_handle: ProcessCancelHandle,
    /// 项目文件夹以外这个任务能写的文件夹（项目的「能改」的其他文件夹，v0.7）。
    pub writable: Vec<PathBuf>,
}

impl ActiveRunContext {
    /// 这个任务会写的文件夹和 `folders` 有没有重叠（一样、或者一个在另一个里面）：
    /// 重叠的两个任务会共用一个影子仓库、互相算进对方的改动，不让同时跑。
    pub fn touches(&self, folders: &[PathBuf]) -> bool {
        std::iter::once(&self.workspace_path)
            .chain(self.writable.iter())
            .any(|a| folders.iter().any(|b| overlaps(a, b)))
    }
}

/// 两个文件夹一样，或者一个在另一个里面（比较前先认真实路径）。
pub fn overlaps(a: &std::path::Path, b: &std::path::Path) -> bool {
    let a = a.canonicalize().unwrap_or_else(|_| a.to_path_buf());
    let b = b.canonicalize().unwrap_or_else(|_| b.to_path_buf());
    a.starts_with(&b) || b.starts_with(&a)
}

