//! 右栏 TERM：站主自己的终端。每条命令在项目根目录起一个 pty 里的 shell，
//! 跑完报退出码和用时；`cd` 到哪儿记下来，下一条命令接着在那儿跑。
//! 环境照站主平时的终端：头一回先用交互式登录 shell 把环境读出来（`user_env`），之后每条命令带着它跑。
//! 不经过权限三档、不进模型上下文；模型在跑任务时不让发新命令（锁在 `commands::term_run` 里判）。

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Serialize;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 命令跑完后把所在目录写到 `PECK_CWD_FILE`，不混进输出。
const SCRIPT: &str = r#"eval "$PECK_COMMAND"
__peck_status=$?
pwd -P > "$PECK_CWD_FILE" 2>/dev/null
exit $__peck_status"#;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload")]
pub enum TermEvent {
    #[serde(rename_all = "camelCase")]
    Output { id: String, chunk: String },
    #[serde(rename_all = "camelCase")]
    Exit {
        id: String,
        code: Option<i32>,
        duration_ms: u64,
        /// 命令跑完后终端所在的目录（相对项目根，根目录是空串；跑到项目外就是绝对路径）。
        cwd: String,
        /// 是站主点 STOP 停掉的。
        stopped: bool,
    },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TermState {
    pub cwd: String,
    /// 正在跑的那条命令的 id。
    pub running: Option<String>,
}

struct Running {
    id: String,
    pid: Option<u32>,
    stopped: Arc<AtomicBool>,
}

struct Session {
    root: PathBuf,
    cwd: PathBuf,
    running: Option<Running>,
}

/// 每个项目一个终端，键是项目 id。只在内存里，退出 App 就没了。
#[derive(Default)]
pub struct Terminals {
    sessions: Mutex<HashMap<String, Session>>,
}

fn display_cwd(root: &Path, cwd: &Path) -> String {
    match cwd.strip_prefix(root) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => cwd.to_string_lossy().to_string(),
    }
}

fn pick_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if Path::new(&shell).is_file() {
            return shell;
        }
    }
    for candidate in ["/bin/zsh", "/bin/bash", "/bin/sh"] {
        if Path::new(candidate).is_file() {
            return candidate.to_string();
        }
    }
    "/bin/sh".to_string()
}

const ENV_MARK: &str = "__PECK_ENV_3f9c__";
const ENV_WAIT: Duration = Duration::from_secs(8);
/// shell 自己管的变量，不从读出来的环境里带。
const ENV_SKIP: [&str; 5] = ["PWD", "OLDPWD", "SHLVL", "_", "TERM"];

/// 从 `<标记>KEY=VALUE\0KEY=VALUE\0<标记>` 里取出环境变量；标记外面是启动脚本自己打的字，不要。
fn parse_env(raw: &[u8]) -> Option<Vec<(String, String)>> {
    let mark = ENV_MARK.as_bytes();
    let find = |hay: &[u8]| hay.windows(mark.len()).position(|w| w == mark);
    let start = find(raw)? + mark.len();
    let end = start + find(&raw[start..])?;
    let vars: Vec<(String, String)> = raw[start..end]
        .split(|b| *b == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (key, value) = entry.split_once('=')?;
            let usable = !key.is_empty()
                && !ENV_SKIP.contains(&key)
                && !key.starts_with("PECK_")
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_');
            usable.then(|| (key.to_string(), value.to_string()))
        })
        .collect();
    vars.iter().any(|(key, _)| key == "PATH").then_some(vars)
}

/// 起一个交互式登录 shell 读环境（`.zprofile`、`.zshrc` 这些都过一遍），最多等 8 秒。
fn read_user_env(shell: &str) -> Option<Vec<(String, String)>> {
    use std::process::{Command, Stdio};
    let script = format!("printf %s {ENV_MARK}; /usr/bin/env -0; printf %s {ENV_MARK}");
    let mut child = Command::new(shell)
        .args(["-l", "-i", "-c", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut raw = Vec::new();
        let _ = stdout.read_to_end(&mut raw);
        let _ = tx.send(raw);
    });
    let raw = rx.recv_timeout(ENV_WAIT).ok();
    if raw.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    parse_env(&raw?)
}

/// 站主平时在终端里的环境。从访达打开的 App 只有系统给的最小 PATH（找不到 pnpm、node 这些），
/// 所以读一次站主自己 shell 的环境，这次打开 App 期间一直用它。读不出来就是 `None`，退回登录 shell。
pub(crate) fn user_env() -> &'static Option<Vec<(String, String)>> {
    static ENV: OnceLock<Option<Vec<(String, String)>>> = OnceLock::new();
    ENV.get_or_init(|| read_user_env(&pick_shell()))
}

/// App 打开时先在后台读一遍，第一条命令就不用等。
pub fn warm_env() {
    std::thread::spawn(|| {
        let _ = user_env();
    });
}

impl Terminals {
    pub fn state(&self, key: &str, root: &Path) -> TermState {
        let sessions = self.sessions.lock().unwrap_or_else(|e| e.into_inner());
        match sessions.get(key) {
            Some(session) => TermState {
                cwd: display_cwd(&session.root, &session.cwd),
                running: session.running.as_ref().map(|r| r.id.clone()),
            },
            None => TermState {
                cwd: display_cwd(root, root),
                running: None,
            },
        }
    }

    pub fn is_running(&self, key: &str) -> bool {
        self.sessions
            .lock()
            .map(|s| s.get(key).is_some_and(|x| x.running.is_some()))
            .unwrap_or(false)
    }

    /// 起一条命令。输出和结束都从 `emit` 出去；返回这条命令的 id。
    pub fn run(
        self: &Arc<Self>,
        key: &str,
        root: &Path,
        command: &str,
        emit: impl Fn(TermEvent) + Send + Sync + 'static,
    ) -> Result<String, String> {
        self.run_with_env(key, root, command, &[], emit)
    }

    /// 同上，另外带上这个项目的环境变量（v0.7，项目设置里记的）。
    pub fn run_with_env(
        self: &Arc<Self>,
        key: &str,
        root: &Path,
        command: &str,
        project_env: &[(String, String)],
        emit: impl Fn(TermEvent) + Send + Sync + 'static,
    ) -> Result<String, String> {
        let command = command.trim();
        if command.is_empty() {
            return Err("先写一条命令。".into());
        }
        let root = root
            .canonicalize()
            .map_err(|_| "项目文件夹不在了，重新打开一次。".to_string())?;
        // 头一回要等 shell 的启动脚本跑完，放在拿锁之前。
        let env = user_env();
        let mut sessions = self.sessions.lock().map_err(|e| e.to_string())?;
        let session = sessions.entry(key.to_string()).or_insert_with(|| Session {
            root: root.clone(),
            cwd: root.clone(),
            running: None,
        });
        if session.running.is_some() {
            return Err("上一条命令还在跑，先点 STOP。".into());
        }
        if session.root != root || !session.cwd.is_dir() {
            session.root = root.clone();
            session.cwd = root.clone();
        }

        let id = uuid::Uuid::new_v4().to_string();
        let cwd_file = std::env::temp_dir().join(format!("peck-term-{id}.cwd"));
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 30,
                cols: 110,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| format!("终端起不来：{e}"))?;
        let mut builder = CommandBuilder::new(pick_shell());
        match env {
            // 环境已经是站主终端里的样子，不再过一遍登录脚本（macOS 的登录脚本会把 PATH 重排）。
            Some(vars) => {
                builder.args(["-c", SCRIPT]);
                for (key, value) in vars {
                    builder.env(key, value);
                }
            }
            None => builder.args(["-l", "-c", SCRIPT]),
        }
        for (key, value) in project_env {
            builder.env(key, value);
        }
        builder.cwd(&session.cwd);
        builder.env("PECK_COMMAND", command);
        builder.env("PECK_CWD_FILE", &cwd_file);
        // 这里只把输出当文字显示，不解释光标和颜色。
        builder.env("TERM", "dumb");
        builder.env("NO_COLOR", "1");
        builder.env("CLICOLOR", "0");
        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|e| format!("命令起不来：{e}"))?;
        drop(pair.slave);
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| format!("终端读不了输出：{e}"))?;

        let stopped = Arc::new(AtomicBool::new(false));
        session.running = Some(Running {
            id: id.clone(),
            pid: child.process_id(),
            stopped: stopped.clone(),
        });
        drop(sessions);

        let emit = Arc::new(emit);
        let (done_tx, done_rx) = mpsc::channel::<()>();
        let reader_emit = emit.clone();
        let reader_id = id.clone();
        std::thread::spawn(move || {
            let mut pending: Vec<u8> = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        pending.extend_from_slice(&buf[..n]);
                        // 汉字可能被切在两次读取之间，留着不完整的尾巴等下一次。
                        let valid = match std::str::from_utf8(&pending) {
                            Ok(_) => pending.len(),
                            Err(e) if e.error_len().is_none() => e.valid_up_to(),
                            Err(_) => pending.len(),
                        };
                        if valid > 0 {
                            let chunk = String::from_utf8_lossy(&pending[..valid]).to_string();
                            pending.drain(..valid);
                            reader_emit(TermEvent::Output {
                                id: reader_id.clone(),
                                chunk,
                            });
                        }
                    }
                }
            }
            if !pending.is_empty() {
                reader_emit(TermEvent::Output {
                    id: reader_id.clone(),
                    chunk: String::from_utf8_lossy(&pending).to_string(),
                });
            }
            let _ = done_tx.send(());
        });

        let terminals = self.clone();
        let key = key.to_string();
        let exit_id = id.clone();
        let started = Instant::now();
        std::thread::spawn(move || {
            let status = child.wait();
            // 主端要留到子进程结束；先等输出读完（后台进程还占着终端时最多等一会儿）。
            let _ = done_rx.recv_timeout(Duration::from_millis(400));
            drop(pair.master);
            let code = status.ok().map(|s| s.exit_code() as i32);
            let mut shown = String::new();
            if let Ok(mut sessions) = terminals.sessions.lock() {
                if let Some(session) = sessions.get_mut(&key) {
                    if let Ok(raw) = std::fs::read_to_string(&cwd_file) {
                        let next = PathBuf::from(raw.trim());
                        if next.is_dir() {
                            session.cwd = next;
                        }
                    }
                    session.running = None;
                    shown = display_cwd(&session.root, &session.cwd);
                }
            }
            let _ = std::fs::remove_file(&cwd_file);
            emit(TermEvent::Exit {
                id: exit_id,
                code,
                duration_ms: started.elapsed().as_millis() as u64,
                cwd: shown,
                stopped: stopped.load(Ordering::SeqCst),
            });
        });
        Ok(id)
    }

    /// STOP：结束正在跑的那条命令和它起的所有进程。
    pub fn stop(&self, key: &str) {
        let pid = {
            let Ok(sessions) = self.sessions.lock() else {
                return;
            };
            let Some(running) = sessions.get(key).and_then(|s| s.running.as_ref()) else {
                return;
            };
            running.stopped.store(true, Ordering::SeqCst);
            running.pid
        };
        let Some(pid) = pid.filter(|p| *p > 0) else {
            return;
        };
        // pty 里的 shell 是会话首进程，进程组号就是它的 pid。
        let group = format!("-{pid}");
        let signal = |name: &str| {
            let _ = std::process::Command::new("/bin/kill")
                .args([name, "--", &group])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        };
        signal("-TERM");
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(800));
            let _ = std::process::Command::new("/bin/kill")
                .args(["-KILL", "--", &group])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(
        terminals: &Arc<Terminals>,
        key: &str,
        root: &Path,
        command: &str,
    ) -> (String, TermEvent) {
        let (tx, rx) = mpsc::channel::<TermEvent>();
        let tx = Mutex::new(tx);
        terminals
            .run(key, root, command, move |event| {
                let _ = tx.lock().unwrap().send(event);
            })
            .unwrap();
        let mut output = String::new();
        loop {
            match rx.recv_timeout(Duration::from_secs(20)).unwrap() {
                TermEvent::Output { chunk, .. } => output.push_str(&chunk),
                exit @ TermEvent::Exit { .. } => return (output, exit),
            }
        }
    }

    #[test]
    fn env_is_read_between_the_marks_and_shell_owned_names_are_left_out() {
        let raw = format!(
            "启动脚本打的字\n{ENV_MARK}PATH=/opt/homebrew/bin:/usr/bin\0PWD=/tmp\0LANG=zh_CN.UTF-8\0PECK_COMMAND=x\0A=b=c\0{ENV_MARK}后面的不要"
        );
        let vars = parse_env(raw.as_bytes()).unwrap();
        assert_eq!(
            vars,
            vec![
                ("PATH".to_string(), "/opt/homebrew/bin:/usr/bin".to_string()),
                ("LANG".to_string(), "zh_CN.UTF-8".to_string()),
                ("A".to_string(), "b=c".to_string()),
            ]
        );
        // 没有标记、或者里面没有 PATH，都当没读到。
        assert!(parse_env(b"PATH=/usr/bin\0").is_none());
        assert!(parse_env(format!("{ENV_MARK}LANG=C\0{ENV_MARK}").as_bytes()).is_none());
    }

    #[test]
    fn the_shell_environment_is_what_the_owner_gets_in_a_terminal() {
        // 真的起一次交互式登录 shell：读得出 PATH，里面至少有 /usr/bin。
        let vars = read_user_env(&pick_shell()).expect("读不出 shell 的环境");
        let path = &vars.iter().find(|(key, _)| key == "PATH").unwrap().1;
        assert!(path.split(':').any(|dir| dir == "/usr/bin"), "{path}");
    }

    #[test]
    fn commands_report_exit_code_and_remember_the_directory() {
        let root = std::env::temp_dir().join(format!("peck-term-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("sub")).unwrap();
        let terminals = Arc::new(Terminals::default());

        let (output, exit) = collect(&terminals, "ws", &root, "echo 你好 && cd sub");
        assert!(output.contains("你好"), "{output}");
        match exit {
            TermEvent::Exit {
                code, cwd, stopped, ..
            } => {
                assert_eq!(code, Some(0));
                assert_eq!(cwd, "sub");
                assert!(!stopped);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(terminals.state("ws", &root).cwd, "sub");

        // 下一条命令接着在 sub 里跑；失败的命令退出码不是 0。
        let (output, exit) = collect(&terminals, "ws", &root, "pwd; exit 3");
        assert!(output.trim_end().ends_with("sub"), "{output}");
        assert!(matches!(exit, TermEvent::Exit { code: Some(3), .. }));
        assert!(!terminals.is_running("ws"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn stop_ends_a_running_command_and_a_second_one_is_refused_meanwhile() {
        let root = std::env::temp_dir().join(format!("peck-term-stop-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let terminals = Arc::new(Terminals::default());
        let (tx, rx) = mpsc::channel::<TermEvent>();
        let tx = Mutex::new(tx);
        terminals
            .run("ws", &root, "sleep 30", move |event| {
                let _ = tx.lock().unwrap().send(event);
            })
            .unwrap();
        assert!(terminals.is_running("ws"));
        assert!(terminals.run("ws", &root, "echo no", |_| {}).is_err());
        std::thread::sleep(Duration::from_millis(300));
        terminals.stop("ws");
        loop {
            if let TermEvent::Exit {
                stopped,
                duration_ms,
                ..
            } = rx.recv_timeout(Duration::from_secs(10)).unwrap()
            {
                assert!(stopped);
                assert!(duration_ms < 10_000);
                break;
            }
        }
        assert!(!terminals.is_running("ws"));
        let _ = std::fs::remove_dir_all(root);
    }
}
