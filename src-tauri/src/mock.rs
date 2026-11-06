use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;

use crate::events::StreamType;
use crate::process::{ProcessOutcome, ProcessOutputChunk, ProcessStatus};

/// Default simulated duration for `mock.toml` (`peck-mock`), 3 seconds total across 3 stages.
pub const DEFAULT_MOCK_TOTAL_DURATION: Duration = Duration::from_secs(3);

/// Execute the built-in `peck-mock` agent asynchronously, streaming stdout/stderr chunks
/// and respecting cancellation and timeout constraints.
pub async fn run_builtin_mock_process(
    args: &[String],
    cwd: &Path,
    total_duration: Duration,
    output_tx: mpsc::UnboundedSender<ProcessOutputChunk>,
    mut cancel_rx: watch::Receiver<bool>,
) -> ProcessOutcome {
    let start = Instant::now();
    let stage_delay = total_duration / 3;

    // Extract optional --prompt and --handoff from args
    let mut prompt = String::from("默认模拟任务");
    let mut handoff: Option<String> = None;
    let mut write_files = false;

    let mut idx = 0;
    while idx < args.len() {
        match args[idx].as_str() {
            "--prompt" if idx + 1 < args.len() => {
                prompt = args[idx + 1].clone();
                idx += 2;
            }
            "--handoff" if idx + 1 < args.len() => {
                handoff = Some(args[idx + 1].clone());
                idx += 2;
            }
            "--write-files" => {
                write_files = true;
                idx += 1;
            }
            _ => {
                idx += 1;
            }
        }
    }

    let send_log = |stream: StreamType, chunk: String| {
        let _ = output_tx.send(ProcessOutputChunk { stream, chunk });
    };

    send_log(
        StreamType::Stdout,
        format!(
            "[peck-mock] Starting simulated agent in workspace: {}",
            cwd.display()
        ),
    );
    send_log(
        StreamType::Stdout,
        format!("[peck-mock] Received prompt: \"{prompt}\""),
    );

    // Stage 1 (1/3 duration)
    tokio::select! {
        _ = sleep(stage_delay) => {}
        _ = cancel_rx.changed() => {
            if *cancel_rx.borrow() {
                send_log(StreamType::Stderr, "[peck-mock] Cancelled during stage 1".to_string());
                return ProcessOutcome {
                    status: ProcessStatus::Cancelled,
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        }
    }

    let handoff_rel = handoff.unwrap_or_else(|| ".peck/handoff/mock".to_string());
    send_log(
        StreamType::Stdout,
        format!("[peck-mock] Stage 1/3: Synthesizing specification -> {handoff_rel}/intent.md"),
    );

    if write_files && cwd.exists() {
        let handoff_dir = cwd.join(&handoff_rel);
        let _ = fs::create_dir_all(&handoff_dir);
        let _ = fs::write(
            handoff_dir.join("intent.md"),
            format!("# Mock Intent Spec\n\nPrompt: {prompt}\n"),
        );
        let peck_dir = cwd.join(".peck");
        let _ = fs::create_dir_all(&peck_dir);
        let _ = fs::write(
            peck_dir.join("tasks.md"),
            "- [x] 生成意图说明文档\n- [ ] 执行模拟代码变更\n- [ ] 完成产物校验\n",
        );
    }

    // Stage 2 (1/3 duration)
    tokio::select! {
        _ = sleep(stage_delay) => {}
        _ = cancel_rx.changed() => {
            if *cancel_rx.borrow() {
                send_log(StreamType::Stderr, "[peck-mock] Cancelled during stage 2".to_string());
                return ProcessOutcome {
                    status: ProcessStatus::Cancelled,
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        }
    }

    send_log(
        StreamType::Stdout,
        "[peck-mock] Stage 2/3: Updating checklist -> .peck/tasks.md (2/3 completed)".to_string(),
    );
    send_log(
        StreamType::Stderr,
        "[peck-mock:notice] Using simulated execution pipeline (no external CLI required)"
            .to_string(),
    );

    if write_files && cwd.exists() {
        let peck_dir = cwd.join(".peck");
        let _ = fs::create_dir_all(&peck_dir);
        let _ = fs::write(
            peck_dir.join("tasks.md"),
            "- [x] 生成意图说明文档\n- [x] 执行模拟代码变更\n- [ ] 完成产物校验\n",
        );
    }

    // Stage 3 (1/3 duration)
    tokio::select! {
        _ = sleep(stage_delay) => {}
        _ = cancel_rx.changed() => {
            if *cancel_rx.borrow() {
                send_log(StreamType::Stderr, "[peck-mock] Cancelled during stage 3".to_string());
                return ProcessOutcome {
                    status: ProcessStatus::Cancelled,
                    exit_code: None,
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }
        }
    }

    if write_files && cwd.exists() {
        let peck_dir = cwd.join(".peck");
        let _ = fs::create_dir_all(&peck_dir);
        let _ = fs::write(
            peck_dir.join("tasks.md"),
            "- [x] 生成意图说明文档\n- [x] 执行模拟代码变更\n- [x] 完成产物校验\n",
        );
    }

    send_log(
        StreamType::Stdout,
        "[peck-mock] Stage 3/3: All tasks marked [x]. Completed cleanly (exit code 0).".to_string(),
    );

    ProcessOutcome {
        status: ProcessStatus::Succeeded,
        exit_code: Some(0),
        duration_ms: start.elapsed().as_millis() as u64,
    }
}
