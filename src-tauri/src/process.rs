//! 子进程的停止：任务的停止开关、整个进程组一起停、CLI 和它生出来的进程一起停（[`Family`]）。
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::watch;

/// Peck 起 CLI 时带的记号（v1.2.1），值每次随机。CLI 生出来的进程都继承它：换了进程组、换了会话（Claude Code 跑命令用
/// `detached`）、父进程先退了也带着。系统自带的程序（`/bin/sleep` 这类）的环境变量读不到，靠别的几条认。
pub(crate) const FAMILY_ENV: &str = "PECK_RUN_GROUP";

/// 一个 CLI 和它生出来的全部进程（v1.2.1）：任务结束时一起停（CLI 起的 dev server、无头浏览器不留下）。认法：
/// - 带这次记号的、在 CLI 进程组里的、收尾前记下的进程树里的；
/// - 被丢下的：父进程是 launchd、进程组的组长已经不在（非交互的 shell 里 `&` 出去、shell 先退了的都这样；在终端里
///   `&` 的自己当组长，不算）、CLI 起了以后才起的、工作目录在 CLI 的工作目录里；
/// - 再加上它们的子孙和它们所在的整个进程组。
pub(crate) struct Family {
    leader: u32,
    token: String,
    cwd: PathBuf,
    since: Instant,
    /// 收尾前记下的进程树：CLI 退了以后它们挂到 launchd 下面，就认不出父子了。
    seen: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Proc {
    pid: u32,
    ppid: u32,
    pgid: u32,
    /// 起了几秒。
    age: u64,
    marked: bool,
}

/// `ps` 的 `etime`（`[[天-]时:]分:秒`）→ 秒。
fn seconds(etime: &str) -> Option<u64> {
    let (days, rest) = etime.split_once('-').map_or(Ok((0, etime)), |(d, r)| d.parse::<u64>().map(|d| (d, r))).ok()?;
    let parts: Vec<u64> = rest.split(':').map(str::parse).collect::<Result<_, _>>().ok()?;
    Some(days * 86_400 + parts.iter().fold(0, |acc, p| acc * 60 + p))
}

/// 这几个进程的工作目录（`lsof`）。
fn cwds(pids: &[u32]) -> Vec<(u32, PathBuf)> {
    let list: Vec<String> = pids.iter().map(u32::to_string).collect();
    let Ok(out) = std::process::Command::new("/usr/sbin/lsof").args(["-a", "-d", "cwd", "-Fn", "-p", &list.join(",")]).stdin(Stdio::null()).stderr(Stdio::null()).output() else { return vec![] };
    let mut found = Vec::new();
    let mut pid = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let (Some(n), Some(p)) = (line.strip_prefix('n'), pid) {
            found.push((p, PathBuf::from(n)));
        }
    }
    found
}

/// 现在的全部进程（`ps`；僵尸不算）。带了 `token` 才读环境变量。
fn processes(token: Option<&str>) -> Vec<Proc> {
    let mut c = std::process::Command::new("/bin/ps");
    c.args(["-A", "-ww", "-o", "pid=,ppid=,pgid=,etime=,stat=,command="]);
    if token.is_some() {
        c.arg("-E");
    }
    let Ok(out) = c.stdin(Stdio::null()).stderr(Stdio::null()).output() else { return vec![] };
    let mark = token.map(|t| format!("{FAMILY_ENV}={t}"));
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut w = line.split_whitespace();
            let (pid, ppid, pgid) = (w.next()?.parse().ok()?, w.next()?.parse().ok()?, w.next()?.parse().ok()?);
            let age = seconds(w.next()?)?;
            if w.next()?.starts_with('Z') {
                return None;
            }
            Some(Proc { pid, ppid, pgid, age, marked: mark.as_deref().is_some_and(|m| line.contains(m)) })
        })
        .collect()
}

impl Family {
    pub(crate) fn new(leader: u32, token: String, cwd: &Path) -> Self {
        Self { leader, token, cwd: cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf()), since: Instant::now(), seen: vec![] }
    }

    /// 收尾前（CLI 还活着）记下它的进程树。
    pub(crate) fn remember(&mut self) {
        let all = processes(None);
        let mut tree = HashSet::from([self.leader]);
        while let Some(p) = all.iter().find(|p| tree.contains(&p.ppid) && !tree.contains(&p.pid)) {
            tree.insert(p.pid);
        }
        self.seen = tree.into_iter().collect();
    }

    /// 这一组现在还活着的进程。Peck 自己和它的进程组不算。
    fn members(&self) -> Vec<u32> {
        let all = processes(Some(&self.token));
        let me = std::process::id();
        let own_group = all.iter().find(|p| p.pid == me).map(|p| p.pgid);
        let ok = |p: &Proc| p.pid > 1 && p.pid != me && p.pgid > 1 && Some(p.pgid) != own_group;
        let mut set: HashSet<u32> = all
            .iter()
            .filter(|p| ok(p) && (p.marked || p.pid == self.leader || p.pgid == self.leader || self.seen.contains(&p.pid)))
            .map(|p| p.pid)
            .collect();
        let window = self.since.elapsed().as_secs() + 1;
        let dropped: Vec<u32> = all
            .iter()
            .filter(|p| ok(p) && p.ppid == 1 && p.pgid != p.pid && p.age <= window && !set.contains(&p.pid) && !all.iter().any(|q| q.pid == p.pgid))
            .map(|p| p.pid)
            .collect();
        if !dropped.is_empty() {
            set.extend(cwds(&dropped).into_iter().filter(|(_, cwd)| cwd.starts_with(&self.cwd)).map(|(pid, _)| pid));
        }
        loop {
            let groups: HashSet<u32> = all.iter().filter(|p| set.contains(&p.pid)).map(|p| p.pgid).collect();
            let more: Vec<u32> = all.iter().filter(|p| ok(p) && !set.contains(&p.pid) && (set.contains(&p.ppid) || groups.contains(&p.pgid))).map(|p| p.pid).collect();
            if more.is_empty() {
                break;
            }
            set.extend(more);
        }
        set.into_iter().collect()
    }

    /// 任务结束：先 TERM，等一会儿还在的（连这期间新起的）KILL。
    pub(crate) async fn stop(self) {
        let family = std::sync::Arc::new(self);
        let scan = |f: std::sync::Arc<Family>| async move { tokio::task::spawn_blocking(move || f.members()).await.unwrap_or_default() };
        let found = scan(family.clone()).await;
        if found.is_empty() {
            return;
        }
        signal("TERM", &found);
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(150)).await;
            if scan(family.clone()).await.is_empty() {
                return;
            }
        }
        signal("KILL", &scan(family).await);
    }

    /// 来不及等（跑任务的 future 被丢掉）：直接 KILL。
    pub(crate) fn kill_now(&self) {
        signal("KILL", &self.members());
    }
}

fn signal(sig: &str, pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    let _ = std::process::Command::new("/bin/kill")
        .arg(format!("-{sig}"))
        .args(pids.iter().map(u32::to_string))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

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

#[cfg(test)]
mod tests {
    use crate::gateway::engines::{Launch, Pipe};
    use std::time::Duration;

    fn alive(tag: &str) -> bool {
        let out = std::process::Command::new("/bin/ps").args(["-A", "-ww", "-o", "stat=,command="]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).lines().any(|l| !l.trim_start().starts_with('Z') && l.contains(tag))
    }

    /// 假 CLI：同组的 `sleep`、`setsid` 出去的 `sleep`（父进程还活着）、被丢下的 `sleep`，有 node 的话再起一个两层 fork
    /// 出去的 node（父进程马上退了，只能靠记号认）。收尾以后都不在了。
    #[tokio::test]
    async fn a_cli_and_everything_it_started_stop_together() {
        let tag = format!("{}", 7000 + std::process::id() % 1000);
        let node = std::process::Command::new("/bin/sh").args(["-lc", "command -v node"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).filter(|n| !n.is_empty());
        let detached = node.as_ref().map(|n| format!("'{n}' -e \"require('child_process').spawn(process.execPath, ['-e', 'setTimeout(()=>{{}}, 9{tag}0)'], {{detached: true, stdio: 'ignore'}}).unref()\"; ")).unwrap_or_default();
        // 被丢下的：新会话里再 fork 一层、外层马上退（像 Claude Code 的后台命令），系统自带的 sleep 读不到记号。
        let dropped = format!("/bin/sh -c 'sleep 3{tag} & exit' & /usr/bin/perl -MPOSIX -e 'POSIX::setsid(); exec \"/bin/sh\", \"-c\", \"sleep 4{tag} & exit\"' & ");
        let script = format!("sleep 1{tag} & /usr/bin/perl -MPOSIX -e 'POSIX::setsid(); exec \"sleep\", \"2{tag}\"' & {dropped}{detached}echo ready; cat");
        let cwd = std::env::temp_dir().join(format!("peck-family-{tag}"));
        std::fs::create_dir_all(&cwd).unwrap();
        let launch = Launch { bin: "/bin/sh".into(), args: vec!["-c".into(), script], cwd: cwd.clone(), env: vec![], path: std::env::var("PATH").unwrap_or_default() };
        let mut pipe = Pipe::spawn(&launch).unwrap();
        assert!(matches!(pipe.next().await, Some(crate::gateway::engines::Line::Text(t)) if t == "ready"));
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert!(alive(&format!("sleep 1{tag}")) && alive(&format!("sleep 2{tag}")) && alive(&format!("sleep 4{tag}")));
        if node.is_some() {
            assert!(alive(&format!("9{tag}0")), "两层 fork 出去的 node 应该在跑");
        }
        pipe.stop(Duration::from_secs(2)).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!alive(&format!("sleep 1{tag}")), "同组的应该停了");
        assert!(!alive(&format!("sleep 2{tag}")), "换了会话的应该停了");
        assert!(!alive(&format!("sleep 3{tag}")) && !alive(&format!("sleep 4{tag}")), "被丢下的应该停了");
        if node.is_some() {
            assert!(!alive(&format!("9{tag}0")), "带记号的应该停了");
        }
        let _ = std::fs::remove_dir(cwd);
    }
}
