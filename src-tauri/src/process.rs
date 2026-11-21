//! 子进程的停止：任务的停止开关、整个进程组一起停（v0.8 删了旧的按清单起 CLI 的那一套）。
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::watch;

/// Handle allowing external callers (e.g. `run_cancel`) to abort a running process group.
#[derive(Clone)]
pub struct ProcessCancelHandle {
    pub(crate) tx: watch::Sender<bool>,
}

impl ProcessCancelHandle {
    pub fn new() -> (Self, watch::Receiver<bool>) {
        let (tx, rx) = watch::channel(false);
        (Self { tx }, rx)
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(true);
    }
}

/// Terminate an entire Unix process group (`-pgid`) as well as the direct child process.
#[cfg(unix)]
pub(crate) async fn terminate_process_group(pgid: Option<u32>) {
    if let Some(id) = pgid {
        if id > 0 {
            let neg_pgid = format!("-{id}");
            let _ = Command::new("/bin/kill")
                .args(["-TERM", "--", &neg_pgid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            let _ = Command::new("/bin/kill")
                .args(["-KILL", "--", &neg_pgid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    }
}

#[cfg(not(unix))]
pub(crate) async fn terminate_process_group(_pgid: Option<u32>) {}

/// 进程组的保险：跑命令的 future 被丢掉（`abort`、被打断）时走不到 [`terminate_process_group`]，
/// `kill_on_drop` 只杀直接的子进程；这里在丢掉时把整组杀掉（cargo 起的 rustc、npm test 起的 worker 不留下）。
pub(crate) struct GroupGuard(Option<u32>);

impl GroupGuard {
    pub(crate) fn new(pgid: Option<u32>) -> Self {
        Self(pgid.filter(|p| *p > 0))
    }

    /// 已经正常收掉了：不用再杀（进程组号可能被别人复用）。
    pub(crate) fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for GroupGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(id) = self.0.take() {
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &format!("-{id}")])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }
}
