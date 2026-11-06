use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use crate::agent::{build_augmented_path, AgentManifest, AgentTemplateVars};
use crate::events::StreamType;
use crate::mock::{run_builtin_mock_process, DEFAULT_MOCK_TOTAL_DURATION};

/// How stdin should be supplied to the spawned CLI process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StdinMode {
    None,
    Prompt(String),
}

/// Complete specification for launching a single Agent CLI process.
#[derive(Debug, Clone)]
pub struct ProcessSpec {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub stdin: StdinMode,
    pub timeout: Duration,
    pub extra_paths: Vec<String>,
    pub env: HashMap<String, String>,
    pub mock_duration: Option<Duration>,
}

impl ProcessSpec {
    /// Construct a `ProcessSpec` from an `AgentManifest` and template variables.
    pub fn from_manifest(manifest: &AgentManifest, vars: &AgentTemplateVars) -> Self {
        let stdin = if manifest.stdin.eq_ignore_ascii_case("prompt") {
            StdinMode::Prompt(vars.prompt.clone())
        } else {
            StdinMode::None
        };

        Self {
            command: manifest.command.clone(),
            args: manifest.render_args(vars),
            cwd: manifest.render_cwd(vars),
            stdin,
            timeout: manifest.timeout_duration(),
            extra_paths: manifest.path.clone(),
            env: manifest.env.clone(),
            mock_duration: None,
        }
    }
}

/// A single line/chunk of output emitted by a running child process.
#[derive(Debug, Clone)]
pub struct ProcessOutputChunk {
    pub stream: StreamType,
    pub chunk: String,
}

/// Terminal state of a completed process execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    Succeeded,
    Failed,
    TimedOut,
    Cancelled,
}

/// Result returned when a child process finishes, fails, times out, or is cancelled.
#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    pub status: ProcessStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

/// Handle allowing external callers (e.g. `run_cancel`) to abort a running process group.
#[derive(Clone)]
pub struct ProcessCancelHandle {
    tx: watch::Sender<bool>,
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
async fn terminate_process_group(pgid: Option<u32>) {
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
async fn terminate_process_group(_pgid: Option<u32>) {}

/// Spawn and supervise a child CLI process (or built-in `peck-mock`), streaming
/// stdout/stderr lines in real time through `output_tx`, with timeout and process-group kill.
pub async fn run_process_streaming(
    spec: ProcessSpec,
    output_tx: mpsc::UnboundedSender<ProcessOutputChunk>,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<ProcessOutcome, String> {
    if spec.command == "peck-mock" {
        let duration = spec.mock_duration.unwrap_or(DEFAULT_MOCK_TOTAL_DURATION);
        let outcome = tokio::time::timeout(
            spec.timeout,
            run_builtin_mock_process(&spec.args, &spec.cwd, duration, output_tx, cancel_rx),
        )
        .await;

        return match outcome {
            Ok(res) => Ok(res),
            Err(_) => Ok(ProcessOutcome {
                status: ProcessStatus::TimedOut,
                exit_code: None,
                duration_ms: spec.timeout.as_millis() as u64,
            }),
        };
    }

    let start = Instant::now();
    let augmented_path = build_augmented_path(&spec.extra_paths);

    let mut cmd = Command::new(&spec.command);
    cmd.args(&spec.args)
        .env("PATH", &augmented_path)
        .envs(&spec.env)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    cmd.current_dir(&spec.cwd);

    match &spec.stdin {
        StdinMode::None => {
            cmd.stdin(Stdio::null());
        }
        StdinMode::Prompt(_) => {
            cmd.stdin(Stdio::piped());
        }
    }

    // Isolate the child into its own process group (PGID == child PID) so we can terminate
    // the entire process tree cleanly on cancellation or timeout.
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn command '{}': {e}", spec.command))?;

    let pgid = child.id();

    if let StdinMode::Prompt(prompt_text) = &spec.stdin {
        if let Some(mut stdin_pipe) = child.stdin.take() {
            let bytes = prompt_text.as_bytes().to_vec();
            tokio::spawn(async move {
                let _ = stdin_pipe.write_all(&bytes).await;
                let _ = stdin_pipe.shutdown().await;
            });
        }
    }

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture child stdout".to_string())?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture child stderr".to_string())?;

    let stdout_tx = output_tx.clone();
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout_pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = stdout_tx.send(ProcessOutputChunk {
                stream: StreamType::Stdout,
                chunk: line,
            });
        }
    });

    let stderr_tx = output_tx.clone();
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr_pipe).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = stderr_tx.send(ProcessOutputChunk {
                stream: StreamType::Stderr,
                chunk: line,
            });
        }
    });

    let timeout_sleep = tokio::time::sleep(spec.timeout);
    tokio::pin!(timeout_sleep);

    let (final_status, exit_code) = tokio::select! {
        wait_res = child.wait() => {
            match wait_res {
                Ok(status) => {
                    let code = status.code();
                    let proc_status = if status.success() {
                        ProcessStatus::Succeeded
                    } else {
                        ProcessStatus::Failed
                    };
                    (proc_status, code)
                }
                Err(e) => {
                    return Err(format!("Failed waiting on child process: {e}"));
                }
            }
        }
        _ = &mut timeout_sleep => {
            terminate_process_group(pgid).await;
            let _ = child.kill().await;
            let _ = child.wait().await;
            (ProcessStatus::TimedOut, None)
        }
        changed = cancel_rx.changed() => {
            if changed.is_ok() && *cancel_rx.borrow() {
                terminate_process_group(pgid).await;
                let _ = child.kill().await;
                let _ = child.wait().await;
                (ProcessStatus::Cancelled, None)
            } else {
                let status = child.wait().await.map_err(|e| e.to_string())?;
                let proc_status = if status.success() {
                    ProcessStatus::Succeeded
                } else {
                    ProcessStatus::Failed
                };
                (proc_status, status.code())
            }
        }
    };

    // Wait briefly for stdout/stderr reader tasks to drain remaining buffered lines
    let _ = tokio::time::timeout(Duration::from_millis(500), stdout_task).await;
    let _ = tokio::time::timeout(Duration::from_millis(500), stderr_task).await;

    Ok(ProcessOutcome {
        status: final_status,
        exit_code,
        duration_ms: start.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_process_captures_stdout_and_stderr_streams() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let spec = ProcessSpec {
            command: "/bin/sh".to_string(),
            args: vec![
                "-c".to_string(),
                "echo 'hello stdout 1'; echo 'warn stderr 1' >&2; echo 'hello stdout 2'"
                    .to_string(),
            ],
            cwd: std::env::temp_dir(),
            stdin: StdinMode::None,
            timeout: Duration::from_secs(5),
            extra_paths: vec![],
            env: HashMap::new(),
            mock_duration: None,
        };

        let outcome = run_process_streaming(spec, tx, cancel_rx)
            .await
            .expect("process should execute");

        assert_eq!(outcome.status, ProcessStatus::Succeeded);
        assert_eq!(outcome.exit_code, Some(0));

        let mut stdout_lines = Vec::new();
        let mut stderr_lines = Vec::new();
        while let Ok(item) = rx.try_recv() {
            match item.stream {
                StreamType::Stdout => stdout_lines.push(item.chunk),
                StreamType::Stderr => stderr_lines.push(item.chunk),
            }
        }

        assert_eq!(stdout_lines, vec!["hello stdout 1", "hello stdout 2"]);
        assert_eq!(stderr_lines, vec!["warn stderr 1"]);
    }

    #[tokio::test]
    async fn test_process_stdin_prompt_delivery() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let spec = ProcessSpec {
            command: "/bin/cat".to_string(),
            args: vec![],
            cwd: std::env::temp_dir(),
            stdin: StdinMode::Prompt("prompt via stdin\nsecond line\n".to_string()),
            timeout: Duration::from_secs(5),
            extra_paths: vec![],
            env: HashMap::new(),
            mock_duration: None,
        };

        let outcome = run_process_streaming(spec, tx, cancel_rx)
            .await
            .expect("cat should execute");

        assert_eq!(outcome.status, ProcessStatus::Succeeded);

        let mut collected = Vec::new();
        while let Ok(item) = rx.try_recv() {
            collected.push(item.chunk);
        }
        assert_eq!(collected, vec!["prompt via stdin", "second line"]);
    }

    #[tokio::test]
    async fn test_process_timeout_kills_process_group() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (_cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let spec = ProcessSpec {
            command: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "sleep 10".to_string()],
            cwd: std::env::temp_dir(),
            stdin: StdinMode::None,
            timeout: Duration::from_millis(150),
            extra_paths: vec![],
            env: HashMap::new(),
            mock_duration: None,
        };

        let outcome = run_process_streaming(spec, tx, cancel_rx)
            .await
            .expect("timeout should resolve cleanly");

        assert_eq!(outcome.status, ProcessStatus::TimedOut);
        assert_eq!(outcome.exit_code, None);
    }

    #[tokio::test]
    async fn test_process_cancel_terminates_running_child() {
        let (tx, _rx) = mpsc::unbounded_channel();
        let (cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let spec = ProcessSpec {
            command: "/bin/sh".to_string(),
            args: vec!["-c".to_string(), "sleep 10".to_string()],
            cwd: std::env::temp_dir(),
            stdin: StdinMode::None,
            timeout: Duration::from_secs(10),
            extra_paths: vec![],
            env: HashMap::new(),
            mock_duration: None,
        };

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(120)).await;
            cancel_handle.cancel();
        });

        let outcome = run_process_streaming(spec, tx, cancel_rx)
            .await
            .expect("cancelled process should resolve cleanly");

        assert_eq!(outcome.status, ProcessStatus::Cancelled);
        assert_eq!(outcome.exit_code, None);
    }

    #[tokio::test]
    async fn test_process_builtin_mock_agent_streams_logs() {
        let (tx, mut rx) = mpsc::unbounded_channel();
        let (_cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let spec = ProcessSpec {
            command: "peck-mock".to_string(),
            args: vec![
                "--prompt".to_string(),
                "Test builtin mock agent".to_string(),
                "--handoff".to_string(),
                ".peck/handoff/test".to_string(),
            ],
            cwd: std::env::temp_dir(),
            stdin: StdinMode::None,
            timeout: Duration::from_secs(10),
            extra_paths: vec![],
            env: HashMap::new(),
            // Use 300ms in unit test for fast CI while verifying the 3-stage pipeline;
            // default when None is 3s (DEFAULT_MOCK_TOTAL_DURATION).
            mock_duration: Some(Duration::from_millis(300)),
        };

        let outcome = run_process_streaming(spec, tx, cancel_rx)
            .await
            .expect("builtin mock should succeed");

        assert_eq!(outcome.status, ProcessStatus::Succeeded);
        assert_eq!(outcome.exit_code, Some(0));

        let mut chunks = Vec::new();
        while let Ok(item) = rx.try_recv() {
            chunks.push(item.chunk);
        }
        assert!(chunks.iter().any(|c| c.contains("Stage 1/3")));
        assert!(chunks.iter().any(|c| c.contains("Stage 2/3")));
        assert!(chunks.iter().any(|c| c.contains("Stage 3/3")));
    }
}
