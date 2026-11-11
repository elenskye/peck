use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::agent::{build_augmented_path, resolve_command_path, AgentManifest, AgentTemplateVars};
use crate::events::{
    ChangeSetSummary, EventSink, FileChangeItem, RunEvent, RunStatus, StepDescriptor, StepStatus,
    StreamType,
};
use crate::flow::{DispatchPlan, ResolvedStepPlan};
use crate::process::{
    run_process_streaming, ProcessCancelHandle, ProcessOutputChunk, ProcessSpec, ProcessStatus,
};
use crate::snapshot::{record_run_base, record_step_finish, record_step_start, ShadowRepo};
pub use crate::tasks::parse_workspace_tasks;
use crate::watch::{WorkspaceWatcher, DEFAULT_WATCH_INTERVAL};

/// Saved state of a Run in `AppState`, enabling cancellation and step retry (`run_retry_step`).
#[derive(Clone)]
pub struct ActiveRunContext {
    pub run_id: String,
    pub session_id: String,
    pub workspace_path: PathBuf,
    pub handoff_rel: String,
    pub plan: DispatchPlan,
    pub agents: Vec<AgentManifest>,
    pub cancel_handle: ProcessCancelHandle,
}

/// Execute a `DispatchPlan` sequentially starting at `start_step_index`.
/// - Initializes the Shadow Git bare repo for `workspace_path` and records baseline & per-step snapshots.
/// - Emits `RunStarted` (when `start_step_index == 0`), `StepStarted`, `StepOutput`,
///   `FileTouched`, `TasksUpdated`, `StepFinished`, and `RunFinished`.
/// - Automatically pauses the pipeline if any step fails (exit code != 0 or timeout),
///   leaving subsequent steps unexecuted (`Pending`).
pub async fn execute_run_from_step(
    run_id: String,
    workspace_path: PathBuf,
    handoff_rel: String,
    plan: DispatchPlan,
    agents: Vec<AgentManifest>,
    start_step_index: usize,
    sink: EventSink,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    mock_duration_override: Option<Duration>,
) -> RunStatus {
    let shadow_repo = ShadowRepo::open_or_init(&workspace_path).ok();

    let initial_descriptors: Vec<StepDescriptor> = plan
        .steps
        .iter()
        .map(|s| {
            let st = if s.step_index < start_step_index {
                StepStatus::Succeeded
            } else {
                StepStatus::Pending
            };
            s.to_descriptor(st)
        })
        .collect();

    if start_step_index == 0 {
        if let Some(ref repo) = shadow_repo {
            if let Ok(base_tree) = repo.create_snapshot(&format!("{run_id}-base")) {
                record_run_base(&run_id, repo, base_tree);
            }
        }

        sink.emit(RunEvent::RunStarted {
            run_id: run_id.clone(),
            flow_id: plan.flow_id.clone(),
            flow_name: plan.flow_name.clone(),
            steps: initial_descriptors,
        });
    }

    let mut closing = String::new();
    for step in plan.steps.iter().skip(start_step_index) {
        sink.emit(RunEvent::StepStarted {
            run_id: run_id.clone(),
            step_id: step.id.clone(),
            step_index: step.step_index,
            agent_id: step.agent_id.clone(),
        });

        let (step_result, tail) = execute_single_step(
            &run_id,
            &workspace_path,
            &handoff_rel,
            step,
            &agents,
            shadow_repo.as_ref(),
            &sink,
            cancel_rx.clone(),
            mock_duration_override,
        )
        .await;
        closing = tail;

        match step_result {
            StepStatus::Succeeded => {
                // Continue to next step in the pipeline
            }
            StepStatus::Cancelled => {
                sink.emit(RunEvent::RunFinished {
                    run_id,
                    status: RunStatus::Cancelled,
                    summary: format!("已停止：「{}」这一步没跑完。", step.title),
                });
                return RunStatus::Cancelled;
            }
            StepStatus::Failed | StepStatus::Pending | StepStatus::Running => {
                // Rule 3.2: Pause pipeline immediately on failure, do NOT trigger subsequent steps
                sink.emit(RunEvent::RunFinished {
                    run_id,
                    status: RunStatus::Failed,
                    summary: format!(
                        "「{}」这一步失败了，后面的没跑。点开这一步看输出，修好以后可以重跑。",
                        step.title
                    ),
                });
                return RunStatus::Failed;
            }
        }
    }

    // 跑完的总结：最后一步收尾时说的话（CLI 的最后一段输出）；没有就写一句。
    sink.emit(RunEvent::RunFinished {
        run_id,
        status: RunStatus::Succeeded,
        summary: if closing.trim().is_empty() {
            format!("{} 跑完了。", plan.flow_name)
        } else {
            closing
        },
    });
    RunStatus::Succeeded
}

/// 一步收尾时最后说的话：标准输出的最后一段，最多 24 行、1600 个字。
fn closing_words(lines: &std::collections::VecDeque<String>) -> String {
    let mut picked: Vec<&str> = Vec::new();
    let mut size = 0usize;
    for line in lines.iter().rev() {
        size += line.chars().count() + 1;
        if size > 1600 {
            break;
        }
        picked.push(line);
    }
    picked.reverse();
    picked.join("\n").trim().to_string()
}

#[allow(clippy::too_many_arguments)]
async fn execute_single_step(
    run_id: &str,
    workspace_path: &Path,
    handoff_rel: &str,
    step: &ResolvedStepPlan,
    agents: &[AgentManifest],
    shadow_repo: Option<&ShadowRepo>,
    sink: &EventSink,
    cancel_rx: tokio::sync::watch::Receiver<bool>,
    mock_duration_override: Option<Duration>,
) -> (StepStatus, String) {
    // 1. Capture Shadow Git snapshot BEFORE step execution
    let before_tree = shadow_repo.and_then(|repo| {
        let tree = repo.create_snapshot(&format!("{}-before", step.id)).ok()?;
        record_step_start(run_id, &step.id, repo, tree.clone());
        Some(tree)
    });

    // 2. Start real-time WorkspaceWatcher (emits FileTouched & TasksUpdated during step)
    let watcher = WorkspaceWatcher::start(
        workspace_path.to_path_buf(),
        run_id.to_string(),
        step.id.clone(),
        sink.clone(),
        DEFAULT_WATCH_INTERVAL,
    );

    let vars = AgentTemplateVars {
        prompt: step.rendered_prompt.clone(),
        handoff: handoff_rel.to_string(),
        workspace: workspace_path.to_string_lossy().to_string(),
    };

    let manifest = agents
        .iter()
        .find(|a| a.id.eq_ignore_ascii_case(&step.agent_id))
        .cloned()
        .unwrap_or_else(|| AgentManifest {
            id: step.agent_id.clone(),
            name: step.agent_name.clone(),
            description: String::new(),
            command: "peck-mock".to_string(),
            args: vec![
                "--prompt".to_string(),
                "{{prompt}}".to_string(),
                "--handoff".to_string(),
                "{{handoff}}".to_string(),
                "--write-files".to_string(),
            ],
            cwd: "{{workspace}}".to_string(),
            stdin: "none".to_string(),
            timeout: "5m".to_string(),
            path: vec![],
            env: Default::default(),
        });

    let mut spec = ProcessSpec::from_manifest(&manifest, &vars);
    spec.mock_duration = mock_duration_override;

    let mut execution_notice = "假 Agent：只走一遍流程、写演示文件，不调用真实 AI。".to_string();
    // Missing executables may be simulated; authentication failures are never hidden as success.
    if spec.command != "peck-mock" {
        let aug_path = build_augmented_path(&spec.extra_paths);
        if let Some(resolved) = resolve_command_path(&spec.command, &aug_path).await {
            spec.command = resolved.to_string_lossy().to_string();
            execution_notice =
                "调用本机 CLI；登录、额度或权限出问题任务会停下，修好以后可以重跑这一步。".into();
        } else {
            execution_notice = format!(
                "没检测到 CLI '{}'，这一步用假 Agent 代跑，不调用真实 AI。",
                spec.command
            );
            sink.emit(RunEvent::StepOutput {
                run_id: run_id.to_string(),
                step_id: step.id.clone(),
                stream: StreamType::Stderr,
                chunk: format!(
                    "[peck] 没在 PATH 里找到 CLI '{}'，这一步改用假 Agent（peck-mock）",
                    spec.command
                ),
            });
            spec.command = "peck-mock".to_string();
            spec.args = vec![
                "--prompt".to_string(),
                step.rendered_prompt.clone(),
                "--handoff".to_string(),
                handoff_rel.to_string(),
                "--write-files".to_string(),
            ];
        }
    }

    let execution_mode = if spec.command == "peck-mock" {
        "mock"
    } else {
        "cli"
    };
    sink.emit(RunEvent::StepExecution {
        run_id: run_id.to_string(),
        step_id: step.id.clone(),
        mode: execution_mode.into(),
        notice: execution_notice,
    });

    let (output_tx, mut output_rx) = mpsc::unbounded_channel::<ProcessOutputChunk>();
    let sink_for_logs = sink.clone();
    let run_id_for_logs = run_id.to_string();
    let step_id_for_logs = step.id.clone();

    let forward_task = tokio::spawn(async move {
        let mut approval_blocked = false;
        let mut tail: std::collections::VecDeque<String> = std::collections::VecDeque::new();
        while let Some(chunk) = output_rx.recv().await {
            if chunk.stream == StreamType::Stdout {
                tail.push_back(chunk.chunk.clone());
                if tail.len() > 24 {
                    tail.pop_front();
                }
            }
            // Grok can exit 0 after refusing a tool in headless mode.
            if chunk.stream == StreamType::Stderr
                && chunk.chunk.starts_with("permission required:")
                && chunk.chunk.contains("headless run cannot ask for approval")
            {
                approval_blocked = true;
            }
            sink_for_logs.emit(RunEvent::StepOutput {
                run_id: run_id_for_logs.clone(),
                step_id: step_id_for_logs.clone(),
                stream: chunk.stream,
                chunk: chunk.chunk,
            });
        }
        (approval_blocked, closing_words(&tail))
    });

    let outcome = match run_process_streaming(spec, output_tx, cancel_rx).await {
        Ok(o) => o,
        Err(err) => {
            let _ = watcher.stop_and_flush().await;
            let _ = forward_task.await;
            sink.emit(RunEvent::StepExecution {
                run_id: run_id.to_string(),
                step_id: step.id.clone(),
                mode: execution_mode.into(),
                notice: format!("这一步起不来：{err}。到设置的 CLI 检测里看看清单，再重跑。"),
            });
            sink.emit(RunEvent::StepOutput {
                run_id: run_id.to_string(),
                step_id: step.id.clone(),
                stream: StreamType::Stderr,
                chunk: format!("[peck:error] {err}"),
            });
            sink.emit(RunEvent::StepFinished {
                run_id: run_id.to_string(),
                step_id: step.id.clone(),
                status: StepStatus::Failed,
                exit_code: Some(-1),
                duration_ms: 0,
                changeset: ChangeSetSummary::default(),
            });
            return (StepStatus::Failed, String::new());
        }
    };

    // 3. Stop watcher (runs a final flush pass to emit any last-millisecond FileTouched / TasksUpdated)
    let watcher_touched = watcher.stop_and_flush().await;
    let (approval_blocked, tail) = forward_task.await.unwrap_or((false, String::new()));

    let step_status = match outcome.status {
        ProcessStatus::Succeeded if approval_blocked => StepStatus::Failed,
        ProcessStatus::Succeeded => StepStatus::Succeeded,
        ProcessStatus::Cancelled => StepStatus::Cancelled,
        ProcessStatus::Failed | ProcessStatus::TimedOut => StepStatus::Failed,
    };
    if step_status == StepStatus::Failed {
        sink.emit(RunEvent::StepExecution {
            run_id: run_id.to_string(), step_id: step.id.clone(),
            mode: execution_mode.into(),
            notice: "这一步失败了：点开输出看是登录、额度、参数还是权限的问题；修好以后可以重跑，不会当成成功。".into(),
        });
    }

    // 4. Capture Shadow Git snapshot AFTER step execution and compute diff changeset
    let mut changeset = if let (Some(repo), Some(ref before)) = (shadow_repo, &before_tree) {
        if let Ok(after) = repo.create_snapshot(&format!("{}-after", step.id)) {
            record_step_finish(run_id, &step.id, after.clone());
            repo.diff_changeset(before, &after, &step.id)
                .unwrap_or_default()
        } else {
            ChangeSetSummary::default()
        }
    } else {
        ChangeSetSummary::default()
    };

    // Ensure every file in the Shadow Git changeset has emitted FileTouched
    for file_item in &changeset.files {
        if !watcher_touched.contains(&file_item.path) {
            sink.emit(RunEvent::FileTouched {
                run_id: run_id.to_string(),
                step_id: step.id.clone(),
                path: file_item.path.clone(),
                kind: file_item.kind.clone(),
            });
        }
    }

    let mut step_status = step_status;
    if step_status == StepStatus::Succeeded {
        // Check expected artifacts declared in Flow step `expects`
        for expected_rel in &step.rendered_expects {
            let expected_full = workspace_path.join(expected_rel);
            if expected_full.exists() {
                if !watcher_touched.contains(expected_rel)
                    && !changeset.files.iter().any(|f| &f.path == expected_rel)
                {
                    sink.emit(RunEvent::FileTouched {
                        run_id: run_id.to_string(),
                        step_id: step.id.clone(),
                        path: expected_rel.clone(),
                        kind: "create".to_string(),
                    });
                    changeset.files.push(FileChangeItem {
                        path: expected_rel.clone(),
                        kind: "create".to_string(),
                        additions: 1,
                        deletions: 0,
                        step_id: step.id.clone(),
                    });
                    changeset.total_additions += 1;
                }
            } else {
                step_status = StepStatus::Failed;
                sink.emit(RunEvent::StepExecution {
                    run_id: run_id.to_string(),
                    step_id: step.id.clone(),
                    mode: execution_mode.into(),
                    notice: format!(
                        "这一步该写出 {expected_rel}，但没有。后面的先不跑，看看 CLI 的输出和权限再重跑。"
                    ),
                });
                sink.emit(RunEvent::StepOutput {
                    run_id: run_id.to_string(),
                    step_id: step.id.clone(),
                    stream: StreamType::Stderr,
                    chunk: format!("[peck:warn] 这一步结束了，但没找到该写出的文件：{expected_rel}"),
                });
            }
        }

        // Parse .peck/tasks.md if present and emit final TasksUpdated for the step
        let tasks = parse_workspace_tasks(workspace_path);
        if !tasks.is_empty() {
            let completed = tasks.iter().filter(|t| t.completed).count();
            sink.emit(RunEvent::TasksUpdated {
                run_id: run_id.to_string(),
                step_id: step.id.clone(),
                total: tasks.len(),
                completed,
                items: tasks,
            });
        }
    }

    sink.emit(RunEvent::StepFinished {
        run_id: run_id.to_string(),
        step_id: step.id.clone(),
        status: step_status,
        exit_code: outcome.exit_code,
        duration_ms: outcome.duration_ms,
        changeset,
    });

    (step_status, tail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::{builtin_default_flows, resolve_dispatch_plan};
    use crate::snapshot::get_diff_for_run;
    use crate::workspace::ensure_peck_in_gitignore;
    use std::collections::HashMap;

    #[tokio::test]
    async fn missing_cli_is_explicitly_simulated_and_missing_artifacts_pause_pipeline() {
        let ws = std::env::temp_dir().join(format!("peck-cli-fallback-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&ws).unwrap();
        let mut agents = crate::agent::builtin_default_manifests();
        agents[0].command = "/nonexistent/peck-test-muse".into();
        let mut plan = resolve_dispatch_plan(
            "run-950001",
            "@muse test",
            ".peck/handoff/950001",
            ws.to_str().unwrap(),
            &agents,
            &builtin_default_flows(),
        )
        .unwrap();
        let (sink, events) = EventSink::new_memory();
        let (_cancel, rx) = ProcessCancelHandle::new();
        let status = execute_run_from_step(
            "run-950001".into(),
            ws.clone(),
            ".peck/handoff/950001".into(),
            plan.clone(),
            agents.clone(),
            0,
            sink,
            rx,
            Some(Duration::from_millis(30)),
        )
        .await;
        assert_eq!(status, RunStatus::Succeeded);
        assert!(ws.join(".peck/handoff/950001/intent.md").exists());
        assert!(events.lock().unwrap().iter().any(|e| matches!(e, RunEvent::StepExecution { mode, notice, .. } if mode == "mock" && notice.contains("没检测到"))));
        agents[0].command = "/bin/sh".into();
        agents[0].args = vec!["-c".into(), "exit 0".into()];
        plan.steps[0].rendered_expects = vec!["missing-artifact.md".into()];
        let (sink, _) = EventSink::new_memory();
        let (_cancel, rx) = ProcessCancelHandle::new();
        let status = execute_run_from_step(
            "run-950002".into(),
            ws.clone(),
            ".peck/handoff/950002".into(),
            plan,
            agents,
            0,
            sink,
            rx,
            None,
        )
        .await;
        assert_eq!(status, RunStatus::Failed);
        std::fs::remove_dir_all(ws).unwrap();
    }

    #[tokio::test]
    async fn test_headless_approval_block_is_failed_even_with_zero_exit() {
        let ws = std::env::temp_dir().join(format!("peck-approval-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&ws).unwrap();
        let mut grok = crate::agent::builtin_default_manifests()
            .into_iter()
            .find(|a| a.id == "grok")
            .unwrap();
        grok.command = "/bin/sh".into();
        grok.args = vec![
            "-c".into(),
            "printf '%s\\n' 'permission required: Execute command was not run, as a headless run cannot ask for approval' >&2; exit 0".into(),
        ];
        let agents = vec![grok];
        let plan = resolve_dispatch_plan(
            "run-950003",
            "@grok implement a feature",
            ".peck/handoff/950003",
            &ws.to_string_lossy(),
            &agents,
            &builtin_default_flows(),
        )
        .unwrap();
        let (sink, events) = EventSink::new_memory();
        let (_cancel, rx) = ProcessCancelHandle::new();
        let status = execute_run_from_step(
            "run-950003".into(),
            ws.clone(),
            ".peck/handoff/950003".into(),
            plan,
            agents,
            0,
            sink,
            rx,
            None,
        )
        .await;
        assert_eq!(status, RunStatus::Failed);
        assert!(events.lock().unwrap().iter().any(|event| matches!(
            event,
            RunEvent::StepFinished {
                status: StepStatus::Failed,
                exit_code: Some(0),
                ..
            }
        )));
        std::fs::remove_dir_all(ws).unwrap();
    }

    #[tokio::test]
    async fn test_run_multi_step_sequential_success() {
        let temp_ws = std::env::temp_dir().join(format!("peck-test-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_ws).unwrap();

        let agents = vec![
            AgentManifest {
                id: "muse".to_string(),
                name: "Muse Wonder".to_string(),
                description: "Step 1 mock".to_string(),
                command: "/bin/sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "mkdir -p .peck/handoff/run-201 && echo '# Intent' > .peck/handoff/run-201/intent.md && printf -- '- [ ] Task 1\\n- [x] Task 2\\n' > .peck/tasks.md && echo 'muse done'".to_string(),
                ],
                cwd: "{{workspace}}".to_string(),
                stdin: "none".to_string(),
                timeout: "10s".to_string(),
                path: vec![],
                env: HashMap::new(),
            },
            AgentManifest {
                id: "grok".to_string(),
                name: "Grok Build".to_string(),
                description: "Step 2 mock".to_string(),
                command: "/bin/sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "printf -- '- [x] Task 1\\n- [x] Task 2\\n' > .peck/tasks.md && echo 'grok done'".to_string(),
                ],
                cwd: "{{workspace}}".to_string(),
                stdin: "none".to_string(),
                timeout: "10s".to_string(),
                path: vec![],
                env: HashMap::new(),
            },
        ];

        let flows = builtin_default_flows();
        let plan = resolve_dispatch_plan(
            "run-201",
            "Add captcha",
            ".peck/handoff/run-201",
            &temp_ws.to_string_lossy(),
            &agents,
            &flows,
        )
        .unwrap();

        let (sink, events_buf) = EventSink::new_memory();
        let (_cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let final_status = execute_run_from_step(
            "run-201".to_string(),
            temp_ws.clone(),
            ".peck/handoff/run-201".to_string(),
            plan,
            agents,
            0,
            sink,
            cancel_rx,
            Some(Duration::from_millis(50)),
        )
        .await;

        assert_eq!(final_status, RunStatus::Succeeded);

        let events = events_buf.lock().unwrap();
        assert!(matches!(events.first(), Some(RunEvent::RunStarted { .. })));
        assert!(matches!(
            events.last(),
            Some(RunEvent::RunFinished {
                status: RunStatus::Succeeded,
                ..
            })
        ));

        // Verify both Step 0 and Step 1 started and finished
        let step_starts: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                RunEvent::StepStarted { step_index, .. } => Some(*step_index),
                _ => None,
            })
            .collect();
        assert_eq!(step_starts, vec![0, 1]);

        let _ = std::fs::remove_dir_all(&temp_ws);
    }

    #[tokio::test]
    async fn test_run_pauses_when_step1_fails_and_skips_step2() {
        let temp_ws = std::env::temp_dir().join(format!("peck-fail-ws-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&temp_ws).unwrap();

        let agents = vec![
            AgentManifest {
                id: "muse".to_string(),
                name: "Muse Wonder".to_string(),
                description: "Failing Step 1".to_string(),
                command: "/bin/sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "echo 'fatal error' >&2; exit 2".to_string(),
                ],
                cwd: "{{workspace}}".to_string(),
                stdin: "none".to_string(),
                timeout: "10s".to_string(),
                path: vec![],
                env: HashMap::new(),
            },
            AgentManifest {
                id: "grok".to_string(),
                name: "Grok Build".to_string(),
                description: "Should not run".to_string(),
                command: "/bin/sh".to_string(),
                args: vec!["-c".to_string(), "echo 'grok should not run'".to_string()],
                cwd: "{{workspace}}".to_string(),
                stdin: "none".to_string(),
                timeout: "10s".to_string(),
                path: vec![],
                env: HashMap::new(),
            },
        ];

        let flows = builtin_default_flows();
        let plan = resolve_dispatch_plan(
            "run-202",
            "Test step 1 failure pause",
            ".peck/handoff/run-202",
            &temp_ws.to_string_lossy(),
            &agents,
            &flows,
        )
        .unwrap();

        let (sink, events_buf) = EventSink::new_memory();
        let (_cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let final_status = execute_run_from_step(
            "run-202".to_string(),
            temp_ws.clone(),
            ".peck/handoff/run-202".to_string(),
            plan,
            agents,
            0,
            sink,
            cancel_rx,
            None,
        )
        .await;

        assert_eq!(final_status, RunStatus::Failed);

        let events = events_buf.lock().unwrap();
        // Verify Step 1 (index 0) started, but Step 2 (index 1) was NEVER started
        let step_starts: Vec<usize> = events
            .iter()
            .filter_map(|e| match e {
                RunEvent::StepStarted { step_index, .. } => Some(*step_index),
                _ => None,
            })
            .collect();
        assert_eq!(step_starts, vec![0]);

        let _ = std::fs::remove_dir_all(&temp_ws);
    }

    #[tokio::test]
    async fn test_run_shadow_git_and_changes_get_diff_e2e() {
        let temp_ws = std::env::temp_dir().join(format!("peck-e2e-diff-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(temp_ws.join("src")).unwrap();
        ensure_peck_in_gitignore(&temp_ws).unwrap();
        std::fs::write(
            temp_ws.join("src/auth.ts"),
            "export const requireCaptcha = false;\n",
        )
        .unwrap();

        let agents = vec![
            AgentManifest {
                id: "muse".to_string(),
                name: "Muse Wonder".to_string(),
                description: "Step 1 intent".to_string(),
                command: "/bin/sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "mkdir -p .peck/handoff/run-203 && printf '# 意图文档\\n开启验证码校验\\n' > .peck/handoff/run-203/intent.md && printf -- '- [x] 生成意图文档\\n- [ ] 修改 auth.ts\\n' > .peck/tasks.md".to_string(),
                ],
                cwd: "{{workspace}}".to_string(),
                stdin: "none".to_string(),
                timeout: "10s".to_string(),
                path: vec![],
                env: HashMap::new(),
            },
            AgentManifest {
                id: "grok".to_string(),
                name: "Grok Build".to_string(),
                description: "Step 2 code".to_string(),
                command: "/bin/sh".to_string(),
                args: vec![
                    "-c".to_string(),
                    "printf 'export const requireCaptcha = true;\\nexport const maxRetries = 5;\\n' > src/auth.ts && printf -- '- [x] 生成意图文档\\n- [x] 修改 auth.ts\\n' > .peck/tasks.md".to_string(),
                ],
                cwd: "{{workspace}}".to_string(),
                stdin: "none".to_string(),
                timeout: "10s".to_string(),
                path: vec![],
                env: HashMap::new(),
            },
        ];

        let flows = builtin_default_flows();
        let plan = resolve_dispatch_plan(
            "run-203",
            "开启登录验证码校验",
            ".peck/handoff/run-203",
            &temp_ws.to_string_lossy(),
            &agents,
            &flows,
        )
        .unwrap();

        let (sink, _events_buf) = EventSink::new_memory();
        let (_cancel_handle, cancel_rx) = ProcessCancelHandle::new();

        let status = execute_run_from_step(
            "run-203".to_string(),
            temp_ws.clone(),
            ".peck/handoff/run-203".to_string(),
            plan,
            agents,
            0,
            sink,
            cancel_rx,
            None,
        )
        .await;
        assert_eq!(status, RunStatus::Succeeded);

        // 1. Verify changes_get_diff (get_diff_for_run) for .peck/handoff/run-203/intent.md
        let intent_diff = get_diff_for_run(
            "run-203",
            Some("run-203-step-intent"),
            Some(".peck/handoff/run-203/intent.md"),
            Some(&temp_ws),
        )
        .unwrap();
        assert_eq!(intent_diff.kind, "create");
        assert_eq!(intent_diff.language, "markdown");
        assert!(intent_diff.unified_diff.contains("+# 意图文档"));
        assert!(intent_diff
            .content_after
            .as_deref()
            .unwrap_or("")
            .contains("开启验证码校验"));

        // 2. Verify changes_get_diff (get_diff_for_run) for src/auth.ts in Step 2
        let code_diff = get_diff_for_run(
            "run-203",
            Some("run-203-step-build"),
            Some("src/auth.ts"),
            Some(&temp_ws),
        )
        .unwrap();
        assert_eq!(code_diff.kind, "modify");
        assert_eq!(code_diff.language, "typescript");
        assert!(code_diff
            .unified_diff
            .contains("-export const requireCaptcha = false;"));
        assert!(code_diff
            .unified_diff
            .contains("+export const requireCaptcha = true;"));
        assert!(code_diff
            .unified_diff
            .contains("+export const maxRetries = 5;"));

        let _ = std::fs::remove_dir_all(&temp_ws);
    }
}
