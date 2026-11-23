//! 多引擎合作（v0.8 Phase 11）：接力（前一个只读、写一份实施规范，后一个照着做）和交叉审查（另一个引擎
//! 只读地看这次的 Diff，说要改就交回实施的那一个修一次）。两样能一起开。
//!
//! 一个任务还是一个任务号、一份任务前快照、一条事件日志（改动记录、回退、独立工作区照常）；每一段是一个
//! 引擎跑一回合：前面一条 `stage.started`，这一段引擎自己的 `turn.completed` 换成 `stage.completed`
//! （`EventSink::for_stage`），整个任务最后一条 `turn.completed`。每一段的引擎可以是 API 模型（Peck Loop）
//! 或 CLI；Antigravity 只做直接调用，不进来（站主 2026-11-12 定）。审批、回答、追加、打断都走同一个
//! 控制通道，转给正在跑的那一段。

use super::context::ProjectLayer;
use super::r#loop::{ContextConfig, Delegate, HistoryProbe, LoopConfig, PeckBackend};
use super::run::{budget, finish_extras, live_spend, memories_of, model_label, permission_of, runtime_dir, snapshot_extras, spill_root, with_project_env};
use crate::events::{ChangeSetSummary, EventSink as RunSink, RunEvent, RunStatus, StepStatus};
use crate::flow::{DispatchPlan, ResolvedStepPlan};
use crate::gateway::event::{AgentEvent, CriterionCheck, EventBody, NoticeLevel, PermissionLevel, ReviewVerdict, StageRole, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{catalog, engines, mcp, providers, runner, AgentBackend, AgentTask, Control, ModelProvider};
use crate::process::ProcessCancelHandle;
use crate::run::ActiveRunContext;
use crate::snapshot::{record_run_base, record_run_finish, ShadowRepo};
use crate::state::AppState;
use crate::store::Store;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::ipc::Channel;
use tokio::sync::mpsc;

/// 审查看的 Diff 最多多大（多了截掉，审查的人自己读文件）。
const REVIEW_DIFF_BYTES: usize = 60_000;

/// 界面选的一个人：谁（`api:…` / `cli:…`）、CLI 里的哪个模型、思考强度。
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamPick {
    pub who: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
}

pub(crate) enum Engine {
    Peck { model_id: String, provider: Arc<dyn ModelProvider> },
    Cli { engine: String, backend: Arc<dyn AgentBackend>, model: Option<String> },
}

/// 合作里的一个人。
pub(crate) struct Member {
    pub who: String,
    pub label: String,
    pub effort: Option<String>,
    pub engine: Engine,
}

impl Member {
    fn engine_id(&self) -> String {
        match &self.engine {
            Engine::Peck { .. } => "peck".into(),
            Engine::Cli { engine, .. } => engine.clone(),
        }
    }

    fn model(&self) -> Option<String> {
        match &self.engine {
            Engine::Peck { model_id, .. } => Some(model_id.clone()),
            Engine::Cli { model, .. } => model.clone(),
        }
    }

    fn is_peck(&self) -> bool {
        matches!(self.engine, Engine::Peck { .. })
    }
}

/// 「谁」→ 一个能跑的人。Antigravity 只做直接调用，不进合作。
pub(crate) async fn member_of(state: &AppState, pick: &TeamPick) -> Result<Member, String> {
    let effort = pick.effort.clone().filter(|e| !e.trim().is_empty() && e != "default");
    if let Some(id) = pick.who.strip_prefix("api:") {
        catalog::catalog().model(id).ok_or_else(|| format!("模型目录里没有 {id}。"))?;
        let provider = providers::for_model(state, id).await.map_err(|e| e.message)?;
        return Ok(Member { who: pick.who.clone(), label: model_label(id), effort, engine: Engine::Peck { model_id: id.to_string(), provider } });
    }
    if let Some(engine) = pick.who.strip_prefix("cli:") {
        if crate::gateway::caps::AgentCaps::for_engine(engine).direct_only {
            return Err("Antigravity 只做直接调用，不进接力和审查。".into());
        }
        let label = catalog::catalog()
            .engines
            .iter()
            .find(|e| e.id == engine)
            .map(|e| e.label.clone())
            .ok_or_else(|| format!("不认识的引擎 {engine}。"))?;
        let backend = engines::from_state(state, engine).await.map_err(|e| e.message)?;
        let model = pick.model.clone().filter(|m| !m.trim().is_empty());
        return Ok(Member { who: pick.who.clone(), label, effort, engine: Engine::Cli { engine: engine.to_string(), backend, model } });
    }
    Err("接力和审查只认 API 模型或 CLI 引擎。".into())
}

/// 发一个合作的任务：`who` 是实施的那一个；`plan_by` 写规范（接力），`review_by` 审查。至少选一样。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn team_run_start(
    session_id: String,
    prompt: String,
    who: String,
    permission: Option<String>,
    model: Option<String>,
    effort: Option<String>,
    cost_cap: Option<f64>,
    worktree: Option<bool>,
    delegate: Option<String>,
    plan_by: Option<TeamPick>,
    review_by: Option<TeamPick>,
    images: Option<Vec<crate::gateway::ImageInput>>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    let level = permission_of(permission.as_deref())?;
    let implement = member_of(&state, &TeamPick { who, model, effort }).await?;
    let spec = match &plan_by {
        Some(p) => Some(member_of(&state, p).await?),
        None => None,
    };
    let review = match &review_by {
        Some(p) => Some(member_of(&state, p).await?),
        None => None,
    };
    // 委派：实施的是 Peck Loop 照旧；是 CLI（v0.9 B1）经本机 MCP（start_team_run 里按这一段准备）。
    let delegate = super::run::delegate_of(&state, delegate.as_deref()).await?;
    let images = super::run::checked_images(images)?;
    start_team_run(
        TeamRun {
            store: state.store.clone(),
            active_runs: state.active_runs.clone(),
            session_id,
            prompt,
            permission,
            level,
            cost_cap,
            env: super::tools::bash::shell_env(),
            worktree: worktree.unwrap_or(false),
            spec,
            implement,
            review,
            delegate,
            images,
        },
        move |e: &AgentEvent| {
            let _ = channel.send(e.clone());
        },
    )
}

pub(crate) struct TeamRun {
    pub store: Arc<Mutex<Store>>,
    pub active_runs: Arc<Mutex<HashMap<String, ActiveRunContext>>>,
    pub session_id: String,
    pub prompt: String,
    pub permission: Option<String>,
    pub level: PermissionLevel,
    pub cost_cap: Option<f64>,
    pub env: Vec<(String, String)>,
    pub worktree: bool,
    pub spec: Option<Member>,
    pub implement: Member,
    pub review: Option<Member>,
    pub delegate: Option<Delegate>,
    /// 这一句附的图（v0.8）：写规范的、实施的看得到（Peck Loop 直接给图，CLI 给存好的文件路径）。
    pub images: Vec<crate::gateway::ImageInput>,
}

/// 记在任务上的「谁」：`team:<写规范>|<实施>|<审查>`（没有的那一格空着）。
pub fn team_who(spec: Option<&str>, implement: &str, review: Option<&str>) -> String {
    format!("team:{}|{implement}|{}", spec.unwrap_or(""), review.unwrap_or(""))
}

/// Peck Loop 那几段共用的东西（每一段按它拼一份 `LoopConfig`）。
#[derive(Clone)]
struct PeckBase {
    store: Arc<Mutex<Store>>,
    workspace_id: String,
    project: ProjectLayer,
    history: HistoryProbe,
    folders: Vec<(PathBuf, bool)>,
    env: Vec<(String, String)>,
    env_names: Vec<String>,
    worktree_of: Option<PathBuf>,
    allowed: HashSet<String>,
    prior_changed: Vec<String>,
    spill_dir: PathBuf,
    runtime_dir: PathBuf,
    cost_cap: Option<f64>,
    mcp: Vec<super::mcp_client::McpServerConfig>,
    /// 几段共用的 MCP 服务器（v0.10）：第一段用到时起，合作做完一起停。
    mcp_slot: super::mcp_client::McpSlot,
    /// 这个任务前几次已经委派了几次（v0.9）。
    delegations_used: u32,
    /// 快照回退、定时任务（v1.0.2）：实施、按审查改那两段才给。
    extras: Arc<super::extras::Extras>,
    /// 整个任务的档位：只读的那两段（写规范、审查）的系统提示里写明它，免得把自己的只读当成任务的权限。
    level: PermissionLevel,
}

impl PeckBase {
    fn config(&self, model_id: &str, read_only: bool, brief: Option<String>, delegate: Option<Delegate>) -> LoopConfig {
        LoopConfig {
            model_id: model_id.to_string(),
            max_rounds: super::run::max_rounds(&self.store),
            budget: budget(&self.store, self.cost_cap),
            live_spend: Some(live_spend(&self.store)),
            oneshot: false,
            allowed: self.allowed.clone(),
            spill_dir: self.spill_dir.clone(),
            runtime_dir: self.runtime_dir.clone(),
            env: self.env.clone(),
            session_ref: None,
            context: ContextConfig {
                project: self.project.clone(),
                environment: true,
                prior_changed: self.prior_changed.clone(),
                refs: vec![],
                history: Some(self.history.clone()),
                compact_at: 0.0,
                folders: if read_only { self.folders.iter().map(|(p, _)| (p.clone(), false)).collect() } else { self.folders.clone() },
                env_names: self.env_names.clone(),
                worktree_of: self.worktree_of.clone(),
            },
            delegate: if read_only { None } else { delegate },
            brief,
            extras: (!read_only).then(|| self.extras.clone()),
            task_level: read_only.then_some(self.level),
            mcp: self.mcp.clone(),
            mcp_slot: Some(self.mcp_slot.clone()),
            mcp_seen: Some(super::mcp_client::seen_hook(self.store.clone(), self.workspace_id.clone())),
            delegations_used: self.delegations_used,
            subtask: None,
        }
    }

    fn backend(&self, provider: Arc<dyn ModelProvider>, config: LoopConfig) -> PeckBackend {
        let store = self.store.clone();
        let ws = self.workspace_id.clone();
        PeckBackend::new(provider, config, vec![]).on_allow_always(move |rule| {
            if let Ok(s) = store.lock() {
                let _ = s.harness_allow(&ws, rule);
            }
        })
    }
}

/// 验收标准最多几条（多了只取前面的；审查逐条判，太多反而判不细）。
const MAX_CRITERIA: usize = 12;

fn spec_prompt(prompt: &str, implementer: &str, level: PermissionLevel) -> String {
    let stage = super::r#loop::read_only_stage_note(level);
    format!(
        "{prompt}\n\n<peck-relay role=\"spec\">\nYou are the first of two agents on this request. Another agent ({implementer}) will implement it after you. Do not change any files: your stage is {stage} Investigate the project as much as you need, then write an implementation spec for that agent, in the user's language, under exactly these five Markdown headings in this order (for a Chinese request: `## 目标`, `## 改哪些文件`, `## 步骤`, `## 验收标准`, `## 不做什么`; otherwise `## Goal`, `## Files to change`, `## Steps`, `## Acceptance criteria`, `## Out of scope`).\nUnder the acceptance criteria heading write a numbered list (1., 2., …) of at most 8 concrete, checkable statements about the finished result: each one something a reviewer can verify from the diff, the files or a command's output (name the command). A separate reviewer will judge the work only against these items, so cover what matters and nothing vague. Be concrete and brief. Your final answer is the spec.\n</peck-relay>"
    )
}

/// 规范里没写验收标准：让写规范的补一次（只要这一节）。
const SPEC_RETRY: &str = "Your spec has no numbered acceptance criteria. Reply with only that section: the heading (`## 验收标准` for a Chinese request, otherwise `## Acceptance criteria`) followed by a numbered list (1., 2., …) of at most 8 concrete, checkable statements about the finished result. Do not change any files.";

fn implement_prompt(prompt: &str, spec: &str, writer: &str) -> String {
    format!(
        "{prompt}\n\n<peck-spec from=\"{writer}\">\n{}\n</peck-spec>\nThe spec above was written by another agent after reading the project. Implement the request following it; a reviewer will check your work against its acceptance criteria one by one. Where the spec is wrong or incomplete, use your judgment and say what you did differently.",
        spec.trim()
    )
}

fn review_prompt(prompt: &str, spec: Option<&str>, criteria: &[String], diff: &str, implementer: &str, level: PermissionLevel) -> String {
    let stage = super::r#loop::read_only_stage_note(level);
    let spec = spec.map(|s| format!("\n<peck-spec>\n{}\n</peck-spec>\n", s.trim())).unwrap_or_default();
    if criteria.is_empty() {
        return format!(
            "Review the changes another agent ({implementer}) just made in this project for this request:\n<peck-request>\n{prompt}\n</peck-request>\n{spec}\n<peck-diff>\n{diff}\n</peck-diff>\nDo not change any files: your stage is {stage} Read files for context when the diff is not enough. Look for bugs, missed requirements, broken edge cases and risky changes. Be specific: file, line, what is wrong, how to fix it. Skip style nits. Write in the user's language. End with exactly one line: `VERDICT: PASS` if the changes are good to keep, or `VERDICT: CHANGES` if the implementer should fix something."
        );
    }
    let list: Vec<String> = criteria.iter().enumerate().map(|(i, c)| format!("{}. {c}", i + 1)).collect();
    format!(
        "Review the changes another agent ({implementer}) just made in this project for this request:\n<peck-request>\n{prompt}\n</peck-request>\n{spec}\n<peck-criteria>\n{}\n</peck-criteria>\n<peck-diff>\n{diff}\n</peck-diff>\nDo not change any files: your stage is {stage} Read files or run read-only commands when the diff is not enough. Judge the changes against each acceptance criterion above, in order, one line each, exactly in this form:\nCHECK 1: PASS — why\nCHECK 2: FAIL — what is wrong and how to fix it\nWrite the reasons in the user's language. After the checks, briefly list any other serious problem (a bug, a risky change) you found, if any. End with exactly one line: `VERDICT: PASS` if every criterion passed, otherwise `VERDICT: CHANGES`.",
        list.join("\n")
    )
}

fn fix_prompt(review: &str, reviewer: &str, failed: &[CriterionCheck]) -> String {
    let review = format!("<peck-review from=\"{reviewer}\">\n{}\n</peck-review>", review.trim());
    if failed.is_empty() {
        return format!("{review}\nAnother agent reviewed your changes (above). Fix the problems it raised that you agree with; for any point you disagree with, say why. Then summarize what you changed.");
    }
    let list: Vec<String> = failed.iter().map(|c| format!("{}. {} — {}", c.n, c.criterion, c.note)).collect();
    format!(
        "{review}\nAnother agent reviewed your changes against the spec's acceptance criteria (above). These criteria did not pass:\n{}\nFix only these; don't redo what already passed. If you disagree with a point, say why. Then summarize what you changed for each one.",
        list.join("\n")
    )
}

/// 一行去掉列表记号、包住整行的加粗或反引号后的样子（行中间的反引号留着）。
fn bare(line: &str) -> &str {
    let mut l = line.trim();
    loop {
        let before = l;
        if let Some(rest) = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).or_else(|| l.strip_prefix("• ")).or_else(|| l.strip_prefix("> ")) {
            l = rest.trim();
        }
        for wrap in ["**", "__", "`"] {
            if l.len() > 2 * wrap.len() && l.starts_with(wrap) && l.ends_with(wrap) {
                l = l[wrap.len()..l.len() - wrap.len()].trim();
            }
        }
        if l == before {
            return l;
        }
    }
}

/// 一行是不是标题（`#` 开头，或整行加粗）。
fn heading(line: &str) -> Option<String> {
    let t = line.trim();
    if t.starts_with('#') {
        return Some(t.trim_start_matches('#').trim().to_lowercase());
    }
    if t.len() > 4 && t.starts_with("**") && t.ends_with("**") {
        return Some(t.trim_matches('*').trim().trim_end_matches([':', '：']).to_lowercase());
    }
    None
}

/// `1. xxx`、`1) xxx`、`1、xxx` → (1, "xxx")。
fn numbered(line: &str) -> Option<(u32, String)> {
    let t = line.trim_start().trim_start_matches(['-', '*']).trim_start();
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() || digits.len() > 2 {
        return None;
    }
    let rest = &t[digits.len()..];
    let rest = rest.strip_prefix(['.', ')', '、', '．']).or_else(|| rest.strip_prefix("）"))?;
    let text = rest.trim();
    (!text.is_empty()).then(|| (digits.parse().unwrap_or(0), text.to_string()))
}

/// 规范里的验收标准：「验收标准」/「Acceptance criteria」那个标题下面编号的条目（续行接到上一条），到下一个标题为止。
pub fn criteria_of(spec: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut inside = false;
    for line in spec.lines() {
        if let Some(h) = heading(line) {
            if inside && !out.is_empty() {
                break;
            }
            inside = h.contains("验收标准") || h.contains("acceptance criteria") || h.contains("验收");
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((_, text)) = numbered(line) {
            out.push(text);
        } else if line.starts_with([' ', '\t']) && !line.trim().is_empty() {
            if let Some(last) = out.last_mut() {
                last.push(' ');
                last.push_str(line.trim());
            }
        }
    }
    out.into_iter()
        .take(MAX_CRITERIA)
        .map(|c| bare(&c).chars().take(400).collect::<String>())
        .filter(|c| !c.is_empty())
        .collect()
}

/// 审查逐条判的结果：`CHECK n: PASS / FAIL — 理由`（也认「通过 / 没过」）。没判的那几条 `pass` 为空。
pub fn checks_of(review: &str, criteria: &[String]) -> Vec<CriterionCheck> {
    let mut found: HashMap<u32, (bool, String)> = HashMap::new();
    for line in review.lines() {
        let l = bare(line);
        let up = l.to_uppercase();
        let Some(rest) = up.strip_prefix("CHECK").map(|_| &l["CHECK".len()..]) else { continue };
        let rest = rest.trim_start().trim_start_matches('#').trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(n) = digits.parse::<u32>() else { continue };
        let rest = rest[digits.len()..].trim_start().trim_start_matches([':', '：', '.']).trim_start();
        let word_up = rest.to_uppercase();
        let (pass, after) = if word_up.starts_with("PASS") {
            (true, &rest[4..])
        } else if word_up.starts_with("FAIL") {
            (false, &rest[4..])
        } else if let Some(r) = rest.strip_prefix("通过") {
            (true, r)
        } else if let Some(r) = rest.strip_prefix("不通过").or_else(|| rest.strip_prefix("没过")) {
            (false, r)
        } else {
            continue;
        };
        let note = after
            .trim_start_matches(|c: char| c.is_ascii_alphabetic())
            .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '—' | '–' | '-' | ':' | '：' | ',' | '，'))
            .trim()
            .to_string();
        found.entry(n).or_insert((pass, note));
    }
    criteria
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let n = i as u32 + 1;
            let (pass, note) = match found.get(&n) {
                Some((p, note)) => (Some(*p), note.clone()),
                None => (None, String::new()),
            };
            CriterionCheck { n, criterion: c.clone(), pass, note }
        })
        .collect()
}

/// 审查的结论：逐条判过的（至少判了一条）看有没有没过的；没逐条判的看最后那行 VERDICT。
pub fn verdict_with(text: &str, checks: &[CriterionCheck]) -> ReviewVerdict {
    if checks.iter().any(|c| c.pass.is_some()) {
        return if checks.iter().any(|c| c.pass == Some(false)) { ReviewVerdict::Changes } else { ReviewVerdict::Pass };
    }
    verdict_of(text)
}

/// 审查的结论：最后几行里的 `VERDICT: PASS / CHANGES`（或中文「结论：通过 / 要改」）。没写当通过。
pub fn verdict_of(text: &str) -> ReviewVerdict {
    // `VERDICT:` 那一行优先，只看冒号后面第一个词（「PASS (no changes needed)」是通过）。
    for line in text.lines().rev().take(8) {
        let l = line.trim().trim_matches(|c| c == '`' || c == '*').trim();
        let up = l.to_uppercase();
        if let Some(i) = up.find("VERDICT") {
            let word: String = up[i + "VERDICT".len()..].trim_start_matches(|c: char| c == ':' || c == '：' || c.is_whitespace() || c == '*' || c == '`').chars().take_while(|c| c.is_ascii_alphabetic()).collect();
            return if word == "CHANGES" { ReviewVerdict::Changes } else { ReviewVerdict::Pass };
        }
    }
    // 没写 VERDICT：看「结论」那一行（不看 `→ ` 开头的下一步建议）。
    for line in text.lines().rev().take(8) {
        let l = line.trim().trim_matches(|c| c == '`' || c == '*').trim();
        if l.starts_with('→') {
            continue;
        }
        if l.contains("结论") {
            return if l.contains("要改") || l.contains("修改") || l.contains("不通过") { ReviewVerdict::Changes } else { ReviewVerdict::Pass };
        }
    }
    ReviewVerdict::Pass
}

/// 一段跑完的样子。
struct StageEnd {
    reason: TurnEndReason,
    error: Option<String>,
    text: String,
    session_ref: Option<String>,
    /// 审查逐条判的结果（带了验收标准才有）。
    checks: Vec<CriterionCheck>,
}

/// 跑一段要的东西。
struct Stager {
    run_id: String,
    cwd: PathBuf,
    level: PermissionLevel,
    base: EventSink,
    current: Arc<Mutex<Option<mpsc::UnboundedSender<Control>>>>,
    last_text: Arc<Mutex<String>>,
    /// 正在跑的这一段的档位（事件出口按它替 CLI 审批）。
    /// 正在跑的这一段是 CLI 时它的档位；Peck Loop 那一段为空——它自己按档位问，规则不碰
    /// （以前也套规则，Peck Loop 的「起常驻进程」确认被批成 `ApproveByRule`，循环不认、一直等；v0.10 收尾审查）。
    stage_level: Arc<Mutex<Option<PermissionLevel>>>,
    /// 实施的是 CLI、选了委派（v0.9 B1）：实施和按审查改那两段的本机 MCP 带 `delegate_task`。
    cli_delegate: Option<Arc<super::delegate::CliDelegate>>,
    /// 项目的环境变量、其他文件夹（v0.10：CLI 那几段也带）。
    project_env: Vec<(String, String)>,
    folders: Vec<(PathBuf, bool)>,
    /// 项目根（v0.10 C3）：CLI 那几段的本机 MCP 带 `create_skill`，提议的 Skill 写进这里。
    skill_root: PathBuf,
    /// 快照回退、定时任务（v1.0.2）：CLI 实施、按审查改的那两段经本机 MCP 用。
    extras: Arc<super::extras::Extras>,
    /// 这个项目里不给的 Peck 工具（v1.1.0：关掉的自带插件的）。
    tools_off: Vec<String>,
}

impl Stager {
    /// 跑一段：发 `stage.started`、换一个控制通道、等这一段收尾，发 `stage.completed`。
    #[allow(clippy::too_many_arguments)]
    async fn run(
        &self,
        member: &Member,
        role: StageRole,
        prompt: String,
        peck: Option<&PeckBackend>,
        resume: Option<String>,
        criteria: &[String],
        title: Option<String>,
    ) -> StageEnd {
        let read_only = matches!(role, StageRole::Spec | StageRole::Review);
        let level = if read_only { PermissionLevel::Plan } else { self.level };
        if let Ok(mut l) = self.stage_level.lock() {
            *l = (!member.is_peck()).then_some(level);
        }
        let engine = member.engine_id();
        self.base.emit(EventBody::StageStarted {
            stage: role,
            engine: engine.clone(),
            model: member.model(),
            title: title.unwrap_or_else(|| format!("{} {}", member.label, role.word())),
        });
        if let Ok(mut t) = self.last_text.lock() {
            t.clear();
        }
        let sink = self.base.for_stage(engine.clone());
        let (tx, rx) = mpsc::unbounded_channel();
        if let Ok(mut c) = self.current.lock() {
            *c = Some(tx);
        }
        let task = AgentTask {
            run_id: self.run_id.clone(),
            prompt,
            cwd: self.cwd.clone(),
            permission: level,
            model: member.model(),
            effort: member.effort.clone(),
            resume,
            peck_mcp: None,
            env: vec![],
            folders: vec![],
        };
        let outcome = match (&member.engine, peck) {
            (Engine::Peck { .. }, Some(backend)) => backend.run(task, sink.clone(), rx).await,
            (Engine::Cli { engine, backend, .. }, _) => {
                let delegate = if matches!(role, StageRole::Implement | StageRole::Fix) { self.cli_delegate.clone() } else { None };
                let extras = (!read_only).then(|| self.extras.clone());
                let offer = mcp::Offer { ask_user: engine == "grok", delegate, skill_root: Some(self.skill_root.clone()), extras, read_only, off: self.tools_off.clone(), git: mcp::GitOffer::of(&self.skill_root, &self.cwd, level) };
                let peck_mcp = mcp::hub().await.ok().map(|h| h.register_offer(sink.clone(), offer));
                let token = peck_mcp.as_ref().map(|m| m.token.clone());
                let prompt = format!("{}{}", task.prompt, runner::folders_note(&self.folders, level));
                let r = backend.run(AgentTask { peck_mcp, prompt, env: self.project_env.clone(), folders: self.folders.clone(), ..task }, sink.clone(), rx).await;
                if let (Some(token), Ok(h)) = (token, mcp::hub().await) {
                    h.unregister(&token);
                }
                r
            }
            (Engine::Peck { .. }, None) => Err(crate::gateway::GatewayError::new(crate::gateway::ErrorKind::Internal, "Peck Loop 没准备好。")),
        };
        if let Ok(mut c) = self.current.lock() {
            *c = None;
        }
        let (reason, error, session_ref) = match outcome {
            Ok(o) => {
                let (r, e) = sink.stage_end().unwrap_or((o.reason, None));
                (r, e, o.session_ref)
            }
            Err(e) => {
                sink.emit(EventBody::Error { message: e.message.clone(), fatal: true });
                (TurnEndReason::Error, Some(e.message), None)
            }
        };
        let text = self.last_text.lock().map(|t| t.clone()).unwrap_or_default();
        let reviewed = role == StageRole::Review && reason == TurnEndReason::Done;
        let checks = if reviewed && !criteria.is_empty() { checks_of(&text, criteria) } else { vec![] };
        let verdict = reviewed.then(|| verdict_with(&text, &checks));
        self.base.emit(EventBody::StageCompleted { stage: role, reason, error: error.clone(), verdict, checks: checks.clone() });
        StageEnd { reason, error, text, session_ref, checks }
    }
}

pub(crate) fn start_team_run(run: TeamRun, out: impl Fn(&AgentEvent) + Send + Sync + 'static) -> Result<String, String> {
    let TeamRun { store, active_runs, session_id, prompt, permission, level, cost_cap, env, worktree, spec, implement, review, delegate, images } = run;
    if spec.is_none() && review.is_none() {
        return Err("接力和审查至少选一样。".into());
    }
    if level == PermissionLevel::Plan {
        return Err("PLAN 只读：接力和审查要动手，选 EDIT 或 AUTO。".into());
    }
    let who = team_who(spec.as_ref().map(|m| m.who.as_str()), &implement.who, review.as_ref().map(|m| m.who.as_str()));
    let title = {
        let mut parts = Vec::new();
        if let Some(s) = &spec {
            parts.push(format!("{} 写规范", s.label));
        }
        parts.push(format!("{} 实施", implement.label));
        if let Some(r) = &review {
            parts.push(format!("{} 审查", r.label));
        }
        parts.join(" → ")
    };
    let settings = {
        let db = store.lock().map_err(|e| e.to_string())?;
        db.workspace_for_session(&session_id).map_err(|e| e.to_string())?.settings
    };
    let env = with_project_env(env, &settings.env_pairs());
    let folders: Vec<(PathBuf, bool)> = if worktree { settings.live_folders().into_iter().map(|(p, _)| (p, false)).collect() } else { settings.live_folders() };
    // 项目的其他文件夹：Peck Loop 和 CLI（v0.10）都认；能改的锁上。
    let writable: Vec<PathBuf> = folders.iter().filter(|(_, w)| *w).map(|(p, _)| p.clone()).collect();
    let project_env = settings.env_pairs();
    let (run_id, step_id, workspace, workspace_id, cancel_rx, refs, fork, earlier, prior_changed) = {
        let mut active = active_runs.lock().map_err(|e| e.to_string())?;
        let mut db = store.lock().map_err(|e| e.to_string())?;
        let ws = db.workspace_for_session(&session_id).map_err(|e| e.to_string())?;
        let workspace = PathBuf::from(&ws.path);
        if !workspace.is_dir() {
            return Err("项目文件夹不在了，重新打开一次。".into());
        }
        crate::worktree::pending_in_session(&db, &session_id)?;
        let mut touching = vec![workspace.clone()];
        touching.extend(writable.iter().cloned());
        if active.values().any(|r| r.touches(&touching)) {
            return Err("同一个项目一次跑一个任务（项目的文件夹、能改的其他文件夹和在跑的任务重叠也算）。".into());
        }
        if worktree && !crate::worktree::available(&workspace) {
            return Err("这个项目不是 git 仓库，开不了独立工作区。".into());
        }
        let refs = super::history::task_refs(&db, &ws.id, &prompt);
        let prior_changed: Vec<String> = db
            .runs(&session_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.undone_at.is_none())
            .flat_map(|r| r.files.into_iter().map(|f| f.path))
            .collect();
        let number = db.next_run_number().map_err(|e| e.to_string())?;
        let run_id = format!("run-{number}");
        let step_id = format!("{run_id}-step-1");
        let plan = DispatchPlan {
            steps: vec![ResolvedStepPlan {
                id: step_id.clone(),
                step_index: 0,
                agent_id: "team".into(),
                agent_name: title.clone(),
                title: title.clone(),
            }],
        };
        let mut record = db.create_run(&session_id, &prompt, number, &plan).map_err(|e| e.to_string())?;
        record.who = Some(who.clone());
        record.permission = permission.clone();
        if worktree {
            let at = crate::worktree::root_for(&workspace).join(&run_id);
            record.worktree = Some(crate::worktree::WorktreeState { path: at.to_string_lossy().into_owned(), applied_at: None, discarded_at: None });
        }
        db.save_run(&record).map_err(|e| e.to_string())?;
        db.set_run_engine(&run_id, "team").map_err(|e| e.to_string())?;
        // 换引擎的那一行照 fork_context；带给每一段的是这个任务前面几次的经过（段都是新会话，不带就什么都不知道）。
        let fork = super::history::fork_context(&db, &session_id, "team", &run_id);
        let earlier = super::history::earlier_context(&db, &session_id, &run_id);
        let (cancel_handle, cancel_rx) = ProcessCancelHandle::new();
        active.insert(
            run_id.clone(),
            ActiveRunContext {
                workspace_path: workspace.clone(),
                cancel_handle,
                writable: writable.clone(),
            },
        );
        (run_id, step_id, workspace, ws.id, cancel_rx, refs, fork, earlier, prior_changed)
    };
    let cwd = super::run::worktree_for(&store, &active_runs, &run_id, &workspace, worktree)?;
    let allowed = store.lock().ok().and_then(|db| db.harness_allowed(&workspace_id).ok()).unwrap_or_default();
    // Peck 替 CLI 审批（和单独发 CLI 一样，runner::rule_allows）：按正在跑的这一段的档位（写规范、审查是 PLAN，不替它批）。
    let stage_level: Arc<Mutex<Option<PermissionLevel>>> = Arc::new(Mutex::new(None));
    let (rule_level, rule_root, rule_allowed) = (stage_level.clone(), cwd.clone(), allowed.clone());
    let rule_hints = store.lock().ok().map(|db| super::mcp_client::hints_for(&db, &workspace_id)).unwrap_or_default();
    // MCP（v0.10 A2）：设置里的 + 项目 .mcp.json 里的，几段共用一组。
    let (mcp_list, mut mcp_notes) = {
        let db = store.lock().map_err(|e| e.to_string())?;
        let ws = db.workspace(&workspace_id).map_err(|e| e.to_string())?;
        super::mcp_client::for_project(&db, &ws)
    };
    let mcp_list = super::mcp_client::fill_secrets(mcp_list, &mut mcp_notes);
    // 这个任务是谁在干（对话里设定时任务时默认照这一套：接力、审查、委派都带着）。
    let pick_of = |m: &Member| TeamPick { who: m.who.clone(), model: if m.is_peck() { None } else { m.model() }, effort: m.effort.clone() };
    let lead = pick_of(&implement);
    let peck_tools = Arc::new(super::extras::Extras {
        store: store.clone(),
        workspace_id: workspace_id.clone(),
        session_id: session_id.clone(),
        run_id: run_id.clone(),
        setup: crate::schedule::Setup { who: lead.who, model: lead.model, effort: lead.effort, plan_by: spec.as_ref().map(pick_of), review_by: review.as_ref().map(pick_of), delegate: delegate.as_ref().map(|d| format!("api:{}", d.model_id)) },
    });
    let tools_off = crate::plugins::tools_off(&settings.plugins_off);
    let peck_base = PeckBase {
        store: store.clone(),
        workspace_id: workspace_id.clone(),
        project: ProjectLayer::load(&workspace, memories_of(&store, &workspace_id)).with_skills(&workspace, &settings),
        history: super::history::probe(store.clone(), workspace_id.clone()),
        folders: folders.clone(),
        env,
        env_names: settings.env.iter().map(|v| v.name.clone()).collect(),
        worktree_of: worktree.then(|| workspace.clone()),
        allowed,
        prior_changed,
        spill_dir: spill_root().join(&session_id),
        runtime_dir: runtime_dir(&workspace),
        cost_cap,
        mcp: mcp_list,
        mcp_slot: Default::default(),
        delegations_used: store.lock().ok().and_then(|db| db.delegations_in_session(&session_id).ok()).unwrap_or(0),
        extras: peck_tools.clone(),
        level,
    };
    // 引擎看到的请求：换引擎带过来的经过、@ 进来的旧任务、站主的原话。
    let mut lead: Vec<String> = Vec::new();
    if let Some(e) = &earlier {
        lead.push(e.text.clone());
    }
    lead.extend(refs);
    let request = if lead.is_empty() { prompt.clone() } else { format!("{}\n\n{prompt}", lead.join("\n\n")) };
    // 附图：CLI 那几段看文件路径，Peck Loop 那几段直接给图。
    let attachments = super::run::save_attachments(&session_id, &images);
    let cli_request = format!("{request}{}", super::run::attachment_note(&attachments));
    let fork_note = fork.as_ref().map(|f| super::history::fork_notice(f, &title));

    let (team_tx, mut team_rx) = mpsc::unbounded_channel::<Control>();
    runner::register_control(&run_id, team_tx.clone());
    let id = run_id.clone();
    let rule_tx = team_tx.clone();
    tokio::spawn(async move {
        let last_text = Arc::new(Mutex::new(String::new()));
        let seen = last_text.clone();
        let base = EventSink::new(id.clone(), Some(step_id.clone()), "team", Some(store.clone()), move |e: &AgentEvent| {
            if let (EventBody::MessageCompleted { text, .. }, None) = (&e.body, &e.subagent_id) {
                if let Ok(mut t) = seen.lock() {
                    *t = text.clone();
                }
            }
            let level = rule_level.lock().ok().and_then(|l| *l);
            // 委派出去的子任务（带 subagent_id）、Peck Loop 那一段是 Peck Loop 自己按档位问的：不替它批、不改卡片。
            if let (None, Some(level)) = (&e.subagent_id, level) {
                if let Some(request_id) = runner::rule_allows(level, &e.body, &rule_root, &rule_allowed) {
                    let _ = rule_tx.send(Control::ApproveByRule { request_id });
                    return;
                }
                match runner::mcp_hint(level, &e.body, &rule_hints, &rule_allowed) {
                    Some(runner::HintAction::Approve(request_id)) => {
                        let _ = rule_tx.send(Control::ApproveByRule { request_id });
                        return;
                    }
                    Some(runner::HintAction::Flag(flagged)) => {
                        out(&AgentEvent { body: flagged, ..e.clone() });
                        return;
                    }
                    None => {}
                }
                if let Some(flagged) = runner::flag_high_risk(&e.body, &rule_root) {
                    out(&AgentEvent { body: flagged, ..e.clone() });
                    return;
                }
            }
            out(e);
        });
        // 控制通道：转给正在跑的那一段；打断记一笔，后面的段不再开。
        let current: Arc<Mutex<Option<mpsc::UnboundedSender<Control>>>> = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));
        let (fwd_current, fwd_stopped) = (current.clone(), stopped.clone());
        let forwarder = tokio::spawn(async move {
            while let Some(c) = team_rx.recv().await {
                if c == Control::Interrupt {
                    fwd_stopped.store(true, Ordering::SeqCst);
                }
                let tx = fwd_current.lock().ok().and_then(|g| g.clone());
                if let Some(tx) = tx {
                    let _ = tx.send(c);
                }
            }
        });
        let interrupt = team_tx.clone();
        let stop_id = id.clone();
        let mut cancel_rx = cancel_rx;
        let watcher = tokio::spawn(async move {
            while cancel_rx.changed().await.is_ok() {
                if *cancel_rx.borrow() {
                    let _ = interrupt.send(Control::Interrupt);
                    // CLI 那一段委派出去的子任务也停（v0.10 收尾审查）。
                    runner::interrupt_subs(&stop_id);
                    break;
                }
            }
        });
        let run_sink = RunSink::new(store.clone());
        let started = Instant::now();
        if let Some(text) = fork_note {
            base.emit(EventBody::Notice { level: NoticeLevel::Info, text });
        }
        for text in mcp_notes {
            base.emit(EventBody::Notice { level: NoticeLevel::Warn, text });
        }
        let mcp_slot = peck_base.mcp_slot.clone();
        let repo = if worktree { crate::worktree::shadow_repo(&cwd, &workspace).ok() } else { ShadowRepo::open_or_init(&workspace).ok() };
        let before = repo.as_ref().and_then(|r| {
            let tree = r.create_snapshot(&format!("{id}-base")).ok()?;
            record_run_base(&id, r, tree.clone());
            Some(tree)
        });
        let extras = snapshot_extras(&id, &folders, level);
        let descriptor = |status| {
            ResolvedStepPlan {
                id: step_id.clone(),
                step_index: 0,
                agent_id: "team".into(),
                agent_name: title.clone(),
                title: title.clone(),
            }
            .to_descriptor(status)
        };
        run_sink.emit(RunEvent::RunStarted { run_id: id.clone(), steps: vec![descriptor(StepStatus::Pending)] });
        run_sink.emit(RunEvent::StepStarted { run_id: id.clone(), step_id: step_id.clone(), step_index: 0, agent_id: "team".into() });
        base.emit(EventBody::TurnStarted { turn_id: format!("turn-{id}") });

        let cli_delegate = match (&implement.engine, delegate.clone()) {
            (Engine::Cli { engine, .. }, Some(d)) if runner::supports_delegate(engine) => {
                super::delegate::prepare(&store, d, &id, &session_id, &cwd, level, cost_cap).ok()
            }
            _ => None,
        };
        let delegate_note = cli_delegate.as_deref().map(super::delegate::prompt_note).unwrap_or_default();
        let stager = Stager { run_id: id.clone(), cwd: cwd.clone(), level, base: base.clone(), current, last_text, stage_level, cli_delegate, project_env: project_env.clone(), folders: folders.clone(), skill_root: workspace.clone(), extras: peck_tools, tools_off };
        let peck_of = |m: &Member, read_only: bool, brief: Option<String>, with_images: bool| -> Option<PeckBackend> {
            match &m.engine {
                Engine::Peck { model_id, provider } => {
                    let d = if read_only { None } else { delegate.clone() };
                    let b = peck_base.backend(provider.clone(), peck_base.config(model_id, read_only, brief, d));
                    Some(if with_images { b.with_images(images.clone()) } else { b })
                }
                Engine::Cli { .. } => None,
            }
        };
        let ask = |m: &Member| if m.is_peck() { request.clone() } else { cli_request.clone() };
        let mut end: (TurnEndReason, Option<String>) = (TurnEndReason::Done, None);
        let mut final_text = String::new();
        let mut spec_text: Option<String> = None;
        // ① 接力：写规范（只读）。
        let mut criteria: Vec<String> = Vec::new();
        if let Some(writer) = &spec {
            let backend = peck_of(writer, true, None, true);
            let first_ask = spec_prompt(&ask(writer), &implement.label, level);
            let r = stager.run(writer, StageRole::Spec, first_ask.clone(), backend.as_ref(), None, &[], None).await;
            if r.reason != TurnEndReason::Done {
                end = (r.reason, r.error.or(Some(format!("{} 没写完规范。", writer.label))));
            } else if r.text.trim().is_empty() {
                end = (TurnEndReason::Error, Some(format!("{} 没写出规范。", writer.label)));
            } else {
                let mut text = r.text.clone();
                criteria = criteria_of(&text);
                // 没写验收标准：让它补一次（Peck Loop 接着原对话；CLI 能续接就续，不能就把草稿带上）。
                if criteria.is_empty() && !stopped.load(Ordering::SeqCst) {
                    let resume = match &writer.engine {
                        Engine::Cli { backend, .. } if backend.caps().resume => r.session_ref.clone(),
                        _ => None,
                    };
                    let again = if writer.is_peck() || resume.is_some() {
                        SPEC_RETRY.to_string()
                    } else {
                        format!("{first_ask}\n\n<peck-spec-draft>\n{}\n</peck-spec-draft>\n{SPEC_RETRY}", text.trim())
                    };
                    let title = Some(format!("{} 补验收标准", writer.label));
                    let more = stager.run(writer, StageRole::Spec, again, backend.as_ref(), resume, &[], title).await;
                    let added = criteria_of(&more.text);
                    if more.reason == TurnEndReason::Done && !added.is_empty() {
                        text = format!("{}\n\n{}", text.trim_end(), more.text.trim());
                        criteria = added;
                    } else {
                        base.emit(EventBody::Notice { level: NoticeLevel::Warn, text: format!("{} 没写出验收标准，审查按整体判。", writer.label) });
                    }
                }
                spec_text = Some(text);
            }
        }
        // ② 实施。
        let impl_backend = peck_of(&implement, false, None, true);
        let mut impl_ref = None;
        if end.0 == TurnEndReason::Done && !stopped.load(Ordering::SeqCst) {
            let text = match (&spec_text, &spec) {
                (Some(s), Some(w)) => implement_prompt(&ask(&implement), s, &w.label),
                _ => ask(&implement),
            };
            let text = format!("{text}{delegate_note}");
            let r = stager.run(&implement, StageRole::Implement, text, impl_backend.as_ref(), None, &[], None).await;
            impl_ref = r.session_ref.clone();
            final_text = r.text.clone();
            if r.reason != TurnEndReason::Done {
                end = (r.reason, r.error);
            }
        }
        // ③ 审查（只读）：看这次到现在的 Diff；要改就交回实施的那一个修一次。
        if let (Some(reviewer), true) = (&review, end.0 == TurnEndReason::Done && !stopped.load(Ordering::SeqCst)) {
            let diff = match (repo.as_ref(), before.as_ref()) {
                (Some(r), Some(b)) => r.create_snapshot(&format!("{id}-review")).map(|t| r.diff_text(b, &t, REVIEW_DIFF_BYTES)).unwrap_or_default(),
                _ => String::new(),
            };
            if diff.trim().is_empty() {
                base.emit(EventBody::Notice { level: NoticeLevel::Info, text: "这次没改文件，不用审查。".into() });
            } else {
                let backend = peck_of(reviewer, true, None, false);
                let r = stager.run(reviewer, StageRole::Review, review_prompt(&request, spec_text.as_deref(), &criteria, &diff, &implement.label, level), backend.as_ref(), None, &criteria, None).await;
                if r.reason != TurnEndReason::Done {
                    end = (r.reason, r.error.or(Some(format!("{} 没审完。", reviewer.label))));
                } else if verdict_with(&r.text, &r.checks) == ReviewVerdict::Changes && !stopped.load(Ordering::SeqCst) {
                    // ④ 按审查改：Peck Loop 接着原来的对话；CLI 能续接就续，不能就把原话带上。
                    let resume = match &implement.engine {
                        Engine::Cli { backend, .. } if backend.caps().resume => impl_ref.clone(),
                        _ => None,
                    };
                    let failed: Vec<CriterionCheck> = r.checks.iter().filter(|c| c.pass == Some(false)).cloned().collect();
                    let text = if implement.is_peck() || resume.is_some() {
                        fix_prompt(&r.text, &reviewer.label, &failed)
                    } else {
                        format!("{request}\n\n{}", fix_prompt(&r.text, &reviewer.label, &failed))
                    };
                    let f = stager.run(&implement, StageRole::Fix, text, impl_backend.as_ref(), resume, &[], None).await;
                    if !f.text.trim().is_empty() {
                        final_text = f.text;
                    }
                    if f.reason != TurnEndReason::Done {
                        end = (f.reason, f.error);
                    }
                }
            }
        }
        if stopped.load(Ordering::SeqCst) && end.0 == TurnEndReason::Done {
            end = (TurnEndReason::Interrupted, None);
        }
        base.emit(EventBody::TurnCompleted { turn_id: format!("turn-{id}"), reason: end.0, error: end.1.clone() });
        watcher.abort();
        forwarder.abort();
        runner::unregister_control(&id);
        super::background::stop_owned(&id);
        if let Some(hub) = mcp_slot.lock().await.take() {
            hub.stop();
        }

        let (step_status, run_status, summary) = match end.0 {
            TurnEndReason::Done => (StepStatus::Succeeded, RunStatus::Succeeded, if final_text.trim().is_empty() { format!("{title}：做完了。") } else { final_text }),
            TurnEndReason::Interrupted => (StepStatus::Cancelled, RunStatus::Cancelled, "已停止。".into()),
            _ => (StepStatus::Failed, RunStatus::Failed, format!("没做完：{}", end.1.unwrap_or_else(|| "出错了。".into()))),
        };
        runner::interrupt_subs(&id);
        let mut changeset = ChangeSetSummary::default();
        if let (Some(r), Some(before)) = (repo.as_ref(), before.as_ref()) {
            if let Ok(after) = r.create_snapshot(&format!("{step_id}-after")) {
                record_run_finish(&id, after.clone());
                changeset = r.diff_changeset(before, &after).unwrap_or_default();
            }
        }
        finish_extras(&id, &extras, &mut changeset);
        // 任务还在跑时批的 Skill（v0.10 C3）：收尾快照拍完了再写，不算进这次的改动。
        crate::skill_propose::flush(&id);
        run_sink.emit(RunEvent::StepFinished {
            run_id: id.clone(),
            step_id: step_id.clone(),
            status: step_status,
            exit_code: None,
            duration_ms: started.elapsed().as_millis() as u64,
            changeset,
        });
        run_sink.emit(RunEvent::RunFinished { run_id: id.clone(), status: run_status, summary });
        if let Ok(mut runs) = active_runs.lock() {
            runs.remove(&id);
        }
    });
    Ok(run_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::ToolKind;
    use crate::gateway::mock::{ScriptTurn, ScriptedModel};
    use crate::gateway::{Part, Role};
    use serde_json::json;
    use std::time::Duration;


    #[test]
    fn read_only_stages_are_told_the_task_is_not_read_only() {
        let s = spec_prompt("做 X", "Codex", PermissionLevel::Auto);
        assert!(s.contains("read-only for this stage only") && s.contains("AUTO permission") && s.contains("don't put a permission mode"), "{s}");
        for criteria in [vec![], vec!["能跑".to_string()]] {
            let r = review_prompt("做 X", None, &criteria, "diff", "Codex", PermissionLevel::Edit);
            assert!(r.contains("read-only for this stage only") && r.contains("EDIT permission"), "{r}");
        }
    }

    #[test]
    fn verdicts_read_the_last_lines() {
        assert_eq!(verdict_of("看了。\n\nVERDICT: CHANGES"), ReviewVerdict::Changes);
        assert_eq!(verdict_of("没问题。\n`VERDICT: PASS`\n"), ReviewVerdict::Pass);
        assert_eq!(verdict_of("**结论：要改**"), ReviewVerdict::Changes);
        assert_eq!(verdict_of("就这样吧"), ReviewVerdict::Pass);
        assert_eq!(verdict_of("VERDICT: PASS (no changes needed)"), ReviewVerdict::Pass);
        assert_eq!(verdict_of("VERDICT: CHANGES\n→ 按结论修改那一处"), ReviewVerdict::Changes);
        assert_eq!(verdict_of("VERDICT: PASS\n→ 结论里说的可以以后修改"), ReviewVerdict::Pass);
        assert_eq!(team_who(Some("cli:codex"), "api:a/b", None), "team:cli:codex|api:a/b|");
    }

    #[test]
    fn criteria_come_from_the_numbered_list_under_the_heading() {
        let spec = "## 目标\n1. 这不是验收标准\n## 验收标准\n1. `a.txt` 写着「二」\n2) 跑 `npm test`\n   全过\n3、不动别的文件\n## 不做什么\n1. 不改样式";
        assert_eq!(criteria_of(spec), vec!["`a.txt` 写着「二」", "跑 `npm test` 全过", "不动别的文件"]);
        let en = "**Acceptance criteria:**\n- 1. Title is `Hi`\n- 2. **Tests pass**\n\n## Out of scope\n- styles";
        assert_eq!(criteria_of(en), vec!["Title is `Hi`", "Tests pass"]);
        assert!(criteria_of("规范：把 notes.txt 改成「新」。").is_empty());
    }

    #[test]
    fn review_checks_decide_the_verdict() {
        let criteria = vec!["标题改了".to_string(), "测试全过".to_string(), "不动别的".to_string()];
        let review = "看了一遍。\n- CHECK 1: PASS — 三个文件都改了\n**CHECK 2: FAIL — `npm test` 有一个失败**\nCHECK 3：通过，只改了 docs\n另外 README 有个错字。\nVERDICT: PASS";
        let checks = checks_of(review, &criteria);
        assert_eq!(checks.iter().map(|c| c.pass).collect::<Vec<_>>(), vec![Some(true), Some(false), Some(true)]);
        assert_eq!(checks[1].note, "`npm test` 有一个失败");
        assert_eq!(checks[2].note, "只改了 docs");
        // 有一条没过：要改（不管最后那行写了什么）。
        assert_eq!(verdict_with(review, &checks), ReviewVerdict::Changes);
        // 都过了：通过，即使写了 CHANGES（别的问题写在正文里给站主看）。
        let ok = "CHECK 1: PASS — 好\nCHECK 2: PASSED — 好\nVERDICT: CHANGES";
        let c = checks_of(ok, &criteria);
        assert_eq!(c[2].pass, None, "没判的那条为空");
        assert_eq!(verdict_with(ok, &c), ReviewVerdict::Pass);
        // 一条都没逐条判：看 VERDICT。
        let free = checks_of("挺好。\nVERDICT: CHANGES", &criteria);
        assert_eq!(verdict_with("挺好。\nVERDICT: CHANGES", &free), ReviewVerdict::Changes);
        let failed: Vec<CriterionCheck> = checks.into_iter().filter(|c| c.pass == Some(false)).collect();
        let fix = fix_prompt(review, "审的", &failed);
        assert!(fix.contains("2. 测试全过 — `npm test` 有一个失败") && !fix.contains("1. 标题改了"));
    }

    fn turn(text: &str, calls: Vec<(&str, serde_json::Value)>) -> ScriptTurn {
        ScriptTurn {
            reasoning: None,
            text: (!text.is_empty()).then(|| text.to_string()),
            tool_calls: calls.into_iter().map(|(n, v)| (n.to_string(), v)).collect(),
            usage: crate::gateway::event::TokenUsage { input: Some(1000), output: Some(100), ..Default::default() },
        }
    }

    fn peck(id: &str, label: &str, turns: Vec<ScriptTurn>) -> Member {
        Member {
            who: format!("api:{id}"),
            label: label.into(),
            effort: None,
            engine: Engine::Peck { model_id: id.into(), provider: Arc::new(ScriptedModel::new(id, turns, Duration::from_millis(1))) },
        }
    }

    #[tokio::test]
    async fn relay_then_review_then_fix_runs_as_one_task() {
        let root = std::env::temp_dir().join(format!("peck-team-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(&ws_dir).unwrap();
        std::fs::write(ws_dir.join("notes.txt"), "旧\n").unwrap();
        let mut store = crate::store::Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w-team".into(),
            path: ws_dir.to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            hidden: false,
            settings: Default::default(),
        };
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        // 快照登记表是整个进程共用的：先占一个大号，免得和别的测试撞号。
        let filler = DispatchPlan { steps: vec![] };
        store.create_run(&session.id, "占号", 950_001, &filler).unwrap();
        let store = Arc::new(Mutex::new(store));
        let model = "deepseek/deepseek-flash".to_string();
        // 第一次没写验收标准，补一次。
        let writer = peck(
            &model,
            "写的",
            vec![
                turn("## 目标\n把 notes.txt 改成「新」。", vec![]),
                turn("## 验收标准\n1. notes.txt 写着「新！」\n2. 不改别的文件", vec![]),
            ],
        );
        let doer = peck(
            &model,
            "做的",
            vec![
                turn("", vec![("read_file", json!({"path": "notes.txt"}))]),
                turn("", vec![("write_file", json!({"path": "notes.txt", "content": "新\n"}))]),
                turn("改好了。", vec![]),
                // 按审查改（新的一回合：先读再写）
                turn("", vec![("read_file", json!({"path": "notes.txt"}))]),
                turn("", vec![("write_file", json!({"path": "notes.txt", "content": "新！\n"}))]),
                turn("按审查加了感叹号。", vec![]),
            ],
        );
        let reviewer = peck(&model, "审的", vec![turn("CHECK 1: FAIL — 少了感叹号\nCHECK 2: PASS — 只改了 notes.txt\nVERDICT: CHANGES", vec![])]);
        let events = Arc::new(Mutex::new(Vec::<AgentEvent>::new()));
        let seen = events.clone();
        let active = Arc::new(Mutex::new(HashMap::new()));
        let run_id = start_team_run(
            TeamRun {
                store: store.clone(),
                active_runs: active.clone(),
                session_id: session.id.clone(),
                prompt: "改一下笔记".into(),
                permission: Some("auto".into()),
                level: PermissionLevel::Auto,
                cost_cap: None,
                env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
                worktree: false,
                spec: Some(writer),
                implement: doer,
                review: Some(reviewer),
                delegate: None,
                images: vec![],
            },
            move |e| seen.lock().unwrap().push(e.clone()),
        )
        .unwrap();
        for _ in 0..400 {
            if active.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(std::fs::read_to_string(ws_dir.join("notes.txt")).unwrap(), "新！\n");
        let events = events.lock().unwrap().clone();
        let stages: Vec<(StageRole, bool)> = events
            .iter()
            .filter_map(|e| match &e.body {
                EventBody::StageCompleted { stage, reason, .. } => Some((*stage, *reason == TurnEndReason::Done)),
                _ => None,
            })
            .collect();
        assert_eq!(stages, vec![(StageRole::Spec, true), (StageRole::Spec, true), (StageRole::Implement, true), (StageRole::Review, true), (StageRole::Fix, true)]);
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::StageStarted { title, .. } if title == "写的 补验收标准")));
        let checks = events.iter().find_map(|e| match &e.body {
            EventBody::StageCompleted { stage: StageRole::Review, checks, .. } => Some(checks.clone()),
            _ => None,
        }).unwrap();
        assert_eq!(checks.len(), 2);
        assert_eq!((checks[0].pass, checks[0].criterion.as_str()), (Some(false), "notes.txt 写着「新！」"));
        // 只有最后一条 turn.completed；写规范、审查是只读的（拿不到写的工具）。
        assert_eq!(events.iter().filter(|e| matches!(e.body, EventBody::TurnCompleted { .. })).count(), 1);
        assert!(matches!(events.last().unwrap().body, EventBody::TurnCompleted { reason: TurnEndReason::Done, .. }));
        let review_verdict = events.iter().find_map(|e| match &e.body {
            EventBody::StageCompleted { stage: StageRole::Review, verdict, .. } => *verdict,
            _ => None,
        });
        assert_eq!(review_verdict, Some(ReviewVerdict::Changes));
        assert!(events.iter().any(|e| matches!(&e.body, EventBody::ToolCallCompleted { .. })));
        let _ = (ToolKind::Edit, Part::Text { text: String::new() }, Role::User);
        let record = store.lock().unwrap().run(&run_id).unwrap();
        assert_eq!(record.status, RunStatus::Succeeded);
        assert!(record.who.as_deref().unwrap().starts_with("team:api:"));
        assert!(record.summary.unwrap().contains("感叹号"));
        assert_eq!(record.files.len(), 1);
        // 再用别家接着发：前面的经过带过去（分叉；占号的那一次也算一次旧的）。
        let fork = super::super::history::fork_context(&store.lock().unwrap(), &session.id, "cli:codex", "run-x").unwrap();
        assert_eq!(fork.runs, 2);
        assert!(fork.text.contains("改一下笔记"));
        // 再来一次合作：上一次合作也是「合作」这一家，但每一段都是新会话，前面的经过照样带上。
        assert!(super::super::history::fork_context(&store.lock().unwrap(), &session.id, "team", "run-x").is_none());
        let earlier = super::super::history::earlier_context(&store.lock().unwrap(), &session.id, "run-x").unwrap();
        assert!(earlier.runs >= 1 && earlier.text.contains("改一下笔记"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
