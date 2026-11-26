//! 发 Peck Loop 的任务（「谁来干」是 `api:<供应商>/<模型>`）和 ONE SHOT。
//!
//! 和 CLI 引擎的任务（`gateway/runner.rs`）一样：任务记录（`runs.engine` 记 `peck`）、一个项目一次一个、
//! 影子仓库的快照（改动记在任务上、能按快照回退）、`run_cancel` 能停、审批和回答走 `gateway_control`。
//! 多出来的：一个任务里的对话存着（下一句接着发）、「这个项目总是允许」存着、花费上限按这个月已花的算。
//!
//! ONE SHOT：只读回答（Peck Loop 按 PLAN 档、只给读的工具；CLI 引擎按 PLAN 档），不占任务号、不锁项目、
//! 不锁终端、不存快照；事件照样进事件日志（花费算进这个月）。

use super::context::ProjectLayer;
use super::r#loop::{Budget, ContextConfig, LoopConfig, PeckBackend, SpendProbe, DEFAULT_MAX_ROUNDS, DEFAULT_ONESHOT_ROUNDS};
use crate::events::{ChangeSetSummary, RunStatus, StepStatus};
use crate::gateway::event::{AgentEvent, EventBody, PermissionLevel, TurnEndReason};
use crate::gateway::log::EventSink;
use crate::gateway::{catalog, engines, providers, runner, AgentBackend, AgentTask, Control, Message, ModelProvider};
use crate::snapshot::ShadowRepo;
use crate::state::AppState;
use crate::store::{now_ms, Store, StoreResult};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::ipc::Channel;
use tokio::sync::mpsc;

pub(crate) fn permission_of(p: Option<&str>) -> Result<PermissionLevel, String> {
    match p {
        None | Some("edit") => Ok(PermissionLevel::Edit),
        Some("plan") => Ok(PermissionLevel::Plan),
        Some("auto") => Ok(PermissionLevel::Auto),
        Some(_) => Err("权限只有 PLAN、EDIT、AUTO 三档。".into()),
    }
}

/// 命令输出落盘的地方：Peck 数据目录下，每个任务（会话）一个文件夹，ONE SHOT 每次一个。
pub fn spill_root() -> PathBuf {
    crate::mode::app_data_dir().join("harness-output")
}

/// 打开 App 时在后台跑：落盘的输出放了 14 天没动过的整个文件夹删掉。
pub fn prune_spills(root: &Path, max_age: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else { return 0 };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age > max_age);
        if old && path.is_dir() && std::fs::remove_dir_all(&path).is_ok() {
            removed += 1;
        }
    }
    removed
}

pub(crate) fn runtime_dir(workspace: &Path) -> PathBuf {
    crate::mode::app_data_dir()
        .join("harness-runtime")
        .join(crate::workspace::workspace_hash(workspace))
}

/// 这个月（UTC）第一天零点的毫秒时间戳。
pub fn month_start_ms(now_ms: i64) -> i64 {
    let now = time::OffsetDateTime::from_unix_timestamp(now_ms / 1000).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    let first = now.date().replace_day(1).unwrap_or(now.date());
    first.midnight().assume_utc().unix_timestamp() * 1000
}

impl Store {
    /// 这个任务里 Peck Loop 的对话（没有就是空的）。
    pub fn harness_conversation(&self, session_id: &str) -> StoreResult<Vec<Message>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT messages FROM harness_conversations WHERE session_id = ?1", params![session_id], |r| r.get(0))
            .optional()?;
        Ok(raw.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default())
    }

    /// 这个任务的 Peck Loop 对话上次是哪个模型（`/compact` 存回去时照旧写它）。
    pub fn harness_conversation_model(&self, session_id: &str) -> StoreResult<String> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT model FROM harness_conversations WHERE session_id = ?1", params![session_id], |r| r.get(0))
            .optional()?;
        Ok(raw.unwrap_or_default())
    }

    pub fn save_harness_conversation(&self, session_id: &str, model: &str, messages: &[Message]) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO harness_conversations(session_id, model, messages, updated_at) VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(session_id) DO UPDATE SET model = excluded.model, messages = excluded.messages, updated_at = excluded.updated_at",
            params![session_id, model, serde_json::to_string(messages)?, now_ms()],
        )?;
        Ok(())
    }

    /// 这个项目「总是允许」过哪些类。
    pub fn harness_allowed(&self, workspace_id: &str) -> StoreResult<HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT rule FROM harness_allow WHERE workspace_id = ?1")?;
        let rows = stmt.query_map(params![workspace_id], |r| r.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// 所有项目「总是允许」过的（设置 → 权限里列出来、能撤）。
    pub fn harness_allow_all(&self) -> StoreResult<Vec<(String, String, i64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT workspace_id, rule, created_at FROM harness_allow ORDER BY created_at DESC")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    pub fn harness_disallow(&self, workspace_id: &str, rule: &str) -> StoreResult<()> {
        self.conn.execute(
            "DELETE FROM harness_allow WHERE workspace_id = ?1 AND rule = ?2",
            params![workspace_id, rule],
        )?;
        Ok(())
    }

    pub fn harness_allow(&self, workspace_id: &str, rule: &str) -> StoreResult<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO harness_allow(workspace_id, rule, created_at) VALUES(?1, ?2, ?3)",
            params![workspace_id, rule, now_ms()],
        )?;
        Ok(())
    }

    /// 从 `since`（毫秒）起 Peck Loop（API）花了多少美元。算不出花费的请求不计。
    pub fn harness_spend_since(&self, since: i64) -> StoreResult<f64> {
        Ok(self.conn.query_row(
            "SELECT COALESCE(SUM(json_extract(event, '$.cost.usd')), 0.0) FROM agent_events
             WHERE (engine = 'peck' OR json_extract(event, '$.model') IS NOT NULL) AND type = 'usage.updated' AND at >= ?1",
            params![since],
            |r| r.get::<_, f64>(0),
        )?)
    }
}

/// 花费上限：上限 + 这个月已经花的。`cap` 空着或 ≤ 0 就不管。
pub(crate) fn budget(store: &Arc<Mutex<Store>>, cap: Option<f64>) -> Option<Budget> {
    let cap = cap.filter(|c| c.is_finite() && *c > 0.0)?;
    let spent = store.lock().ok().and_then(|s| s.harness_spend_since(month_start_ms(now_ms())).ok()).unwrap_or(0.0);
    Some(Budget { cap_usd: cap, spent_usd: spent })
}

/// 每次请求前现算这个月花了多少（花费上限用）。
pub(crate) fn live_spend(store: &Arc<Mutex<Store>>) -> SpendProbe {
    let store = store.clone();
    Arc::new(move || store.lock().ok().and_then(|s| s.harness_spend_since(month_start_ms(now_ms())).ok()))
}

/// 这个项目站主批过的记忆（旧的在前）。
pub(crate) fn memories_of(store: &Arc<Mutex<Store>>, workspace_id: &str) -> Vec<String> {
    store
        .lock()
        .ok()
        .and_then(|s| s.memories(workspace_id).ok())
        .map(|m| m.into_iter().map(|n| n.text).collect())
        .unwrap_or_default()
}

pub(crate) fn model_id_of(who: &str) -> Result<String, String> {
    let id = who.strip_prefix("api:").ok_or("这个入口只跑 Peck Loop（api:…）。")?;
    catalog::catalog().model(id).ok_or_else(|| format!("模型目录里没有 {id}。"))?;
    Ok(id.to_string())
}

pub(crate) fn model_label(model_id: &str) -> String {
    let cat = catalog::catalog();
    match cat.model(model_id) {
        Some(m) => format!("Peck · {}", m.label),
        None => format!("Peck · {model_id}"),
    }
}

/// 发一个 Peck Loop 的任务。返回任务 id；过程经 `channel` 来（统一事件）。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn harness_run_start(
    session_id: String,
    prompt: String,
    who: String,
    permission: Option<String>,
    effort: Option<String>,
    cost_cap: Option<f64>,
    // 独立工作区（v0.7）：在项目的一个 git worktree 里做，做完再搬回。
    worktree: Option<bool>,
    // 这一句附的图（v0.8，站主贴进来的）。
    images: Option<Vec<crate::gateway::ImageInput>>,
    // 这一句附的文档和文件夹（v1.1.8，输入框 + 按钮挑的）。
    uploads: Option<Vec<String>>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    let level = permission_of(permission.as_deref())?;
    let model_id = model_id_of(&who)?;
    let provider = providers::for_model(&state, &model_id).await.map_err(|e| e.message)?;
    // 子代理（v1.1.2）：照设置 → AGENTS 准备好这个任务能派的几类。
    let (team, team_notes) = super::agents::prepare(&state, &model_id).await;
    let images = checked_images(images)?;
    // 附的图也存成文件、路径写在这一句后面（v1.1.11）：对话里照它画小图；对话压缩掉图以后模型还能 read_file 再看。
    let image_paths = save_attachments(&session_id, &images);
    let saved_uploads = save_uploads(&session_id, &uploads.unwrap_or_default())?;
    let prompt = format!("{prompt}{}{}", attachment_note(&image_paths), uploads_note(&saved_uploads));
    start_peck_run(
        PeckRun {
            store: state.store.clone(),
            active_runs: state.active_runs.clone(),
            session_id,
            prompt,
            who,
            permission,
            level,
            model_id,
            effort,
            cost_cap,
            env: super::tools::bash::shell_env(),
            worktree: worktree.unwrap_or(false),
            team: Some(Arc::new(team)),
            team_notes,
            images,
            search: crate::gateway::search::Keys::from_state(&state),
            compact: None,
        },
        provider,
        move |e: &AgentEvent| {
            let _ = channel.send(e.clone());
        },
    )
}

/// 附的图：只认常见的几种，最多 6 张。
pub(crate) fn checked_images(images: Option<Vec<crate::gateway::ImageInput>>) -> Result<Vec<crate::gateway::ImageInput>, String> {
    let images = images.unwrap_or_default();
    if images.len() > 6 {
        return Err("一句话最多带 6 张图。".into());
    }
    images.into_iter().map(|i| i.checked()).collect()
}

/// 附的图存成文件（这个对话的附件目录里），在这一句后面写上路径：CLI 引擎自己读，Peck Loop 图照发、路径留着再看。
pub(crate) fn save_attachments(session_id: &str, images: &[crate::gateway::ImageInput]) -> Vec<PathBuf> {
    let dir = spill_root().join(session_id).join("attachments");
    if images.is_empty() || std::fs::create_dir_all(&dir).is_err() {
        return vec![];
    }
    images
        .iter()
        .filter_map(|i| {
            let ext = i.media_type.strip_prefix("image/").unwrap_or("png").replace("jpeg", "jpg");
            let path = dir.join(format!("{}.{ext}", uuid::Uuid::new_v4().simple()));
            let bytes = crate::files::base64_decode(&i.data)?;
            std::fs::write(&path, bytes).ok()?;
            Some(path)
        })
        .collect()
}

/// 附图的路径写在这一句后面。格式别改：对话里照它把路径画成小图（`src/lib/attachments.ts`）。
pub(crate) fn attachment_note(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return String::new();
    }
    let list: Vec<String> = paths.iter().map(|p| format!("- {}", p.display())).collect();
    format!("\n\n（站主附了 {} 张图，存在这些本机文件里，需要就读：\n{}）", paths.len(), list.join("\n"))
}

/// + 按钮（v1.1.8）：这一句附的文档和文件夹（系统选框挑的本机路径）。
/// 文件拷进附件目录（和附图放一起）；文件夹不拷，只记路径让模型自己进去看。
pub(crate) struct SavedUploads {
    pub files: Vec<PathBuf>,
    pub folders: Vec<PathBuf>,
}

pub(crate) fn save_uploads(session_id: &str, uploads: &[String]) -> Result<SavedUploads, String> {
    const MAX_UPLOADS: usize = 10;
    const MAX_BYTES: u64 = 10 * 1024 * 1024;
    if uploads.len() > MAX_UPLOADS {
        return Err(format!("一次最多附 {MAX_UPLOADS} 个，先挑最要紧的。"));
    }
    let mut saved = SavedUploads { files: vec![], folders: vec![] };
    if uploads.is_empty() {
        return Ok(saved);
    }
    let dir = spill_root().join(session_id).join("attachments");
    std::fs::create_dir_all(&dir).map_err(|e| format!("放不下附的文档：{e}"))?;
    for raw in uploads {
        let path = PathBuf::from(raw);
        if !path.is_absolute() {
            return Err(format!("只收本机选框挑的绝对路径：{raw}"));
        }
        let meta = std::fs::symlink_metadata(&path).map_err(|_| format!("找不到了：{raw}"))?;
        if meta.is_symlink() {
            return Err(format!("链接不收（怕指到外面去）：{raw}"));
        }
        if meta.is_dir() {
            if path.parent().is_none() {
                return Err("根目录太大了，挑小一点的文件夹。".into());
            }
            if std::env::var_os("HOME").is_some_and(|h| h == path.as_os_str()) {
                return Err("家目录太大了，挑小一点的文件夹。".into());
            }
            saved.folders.push(path);
            continue;
        }
        if !meta.is_file() {
            return Err(format!("这个附不上：{raw}"));
        }
        if meta.len() > MAX_BYTES {
            return Err(format!("{} 超过 10MB，拆小一点再附。", path.display()));
        }
        let name = path.file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty() && !n.contains('/')).ok_or_else(|| format!("文件名认不出：{raw}"))?;
        // 同名后到的加序号，不覆盖。
        let mut dest = dir.join(name);
        let mut n = 2;
        while dest.exists() {
            let stem = PathBuf::from(name).file_stem().and_then(|s| s.to_str()).unwrap_or("文档").to_string();
            let ext = PathBuf::from(name).extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            dest = dir.join(format!("{stem}-{n}{ext}"));
            n += 1;
        }
        std::fs::copy(&path, &dest).map_err(|e| format!("拷不进 {}：{e}", path.display()))?;
        saved.files.push(dest);
    }
    Ok(saved)
}

/// 附的文档和文件夹写在这一句后面（CLI 和 Peck Loop 都看）。格式别改：对话里照它画成小片（`src/lib/attachments.ts`）。
pub(crate) fn uploads_note(saved: &SavedUploads) -> String {
    let mut out = String::new();
    if !saved.files.is_empty() {
        let list: Vec<String> = saved.files.iter().map(|p| format!("- {}", p.display())).collect();
        out.push_str(&format!("\n\n（站主附了 {} 个文档，拷在这些本机文件里，需要就读：\n{}）", saved.files.len(), list.join("\n")));
    }
    if !saved.folders.is_empty() {
        let list: Vec<String> = saved.folders.iter().map(|p| format!("- {}", p.display())).collect();
        out.push_str(&format!("\n\n（站主还给了 {} 个文件夹（只读，别改里面；只看要紧的，别全扫）：\n{}）", saved.folders.len(), list.join("\n")));
    }
    out
}

/// 一个任务最多几轮（设置 → GENERAL，默认 40）：手动发的、合作的、定时任务的 Peck Loop 都按它。
/// 关了「每个任务最多几轮」（`roundsUnlimited`，v1.1.12）就不限。
pub(crate) fn max_rounds(store: &Arc<Mutex<Store>>) -> u32 {
    let saved = store.lock().ok().and_then(|db| db.setting("ui").ok().flatten());
    task_rounds_from(saved.as_deref())
}

fn task_rounds_from(saved: Option<&str>) -> u32 {
    let unlimited = saved
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| v.get("roundsUnlimited").and_then(serde_json::Value::as_bool))
        .unwrap_or(false);
    if unlimited {
        super::r#loop::UNLIMITED_ROUNDS
    } else {
        rounds_from(saved, "maxRounds", DEFAULT_MAX_ROUNDS)
    }
}

/// ONE SHOT 最多几轮（v1.0.3 起设置 → GENERAL 能调，默认 12）。
fn oneshot_rounds(store: &Arc<Mutex<Store>>) -> u32 {
    rounds_setting(store, "oneshotRounds", DEFAULT_ONESHOT_ROUNDS)
}

/// 读界面设置里存的轮数，5–1000 之间；没存、存坏了用默认的。
fn rounds_setting(store: &Arc<Mutex<Store>>, key: &str, default: u32) -> u32 {
    let saved = store.lock().ok().and_then(|db| db.setting("ui").ok().flatten());
    rounds_from(saved.as_deref(), key, default)
}

fn rounds_from(saved: Option<&str>, key: &str, default: u32) -> u32 {
    saved
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| v.get(key).and_then(serde_json::Value::as_u64))
        .map(|n| n.clamp(u64::from(super::r#loop::MIN_ROUNDS), u64::from(super::r#loop::MAX_ROUNDS_CAP)) as u32)
        .unwrap_or(default)
}

pub(crate) struct PeckRun {
    pub store: Arc<Mutex<Store>>,
    pub active_runs: crate::run::ActiveRuns,
    pub session_id: String,
    pub prompt: String,
    pub who: String,
    pub permission: Option<String>,
    pub level: PermissionLevel,
    pub model_id: String,
    pub effort: Option<String>,
    pub cost_cap: Option<f64>,
    pub env: Vec<(String, String)>,
    /// 在独立工作区（git worktree）里做。
    pub worktree: bool,
    /// 子代理（v1.1.2）：这个任务能派的几类；`team_notes` 是哪类这次派不了（开头写给站主看）。
    pub team: Option<Arc<super::agents::Team>>,
    pub team_notes: Vec<String>,
    /// 这一句附的图（v0.8）。
    pub images: Vec<crate::gateway::ImageInput>,
    /// 联网搜索手上有什么（v1.1.1）。
    pub search: crate::gateway::search::Keys,
    /// 手动压缩（v1.1.7，`/compact [重点]`）：`Some(重点)` 时不发话，只把存着的对话压一遍。
    pub compact: Option<Option<String>>,
}

/// 命令的环境：站主 shell 的那份 + 这个项目自己的环境变量（同名的项目的算）。
pub(crate) fn with_project_env(mut env: Vec<(String, String)>, project: &[(String, String)]) -> Vec<(String, String)> {
    env.retain(|(k, _)| !project.iter().any(|(p, _)| p == k));
    env.extend(project.iter().cloned());
    env
}

/// 其他文件夹的任务前快照：能改的那几个（PLAN 不改，不拍）。
pub(crate) fn snapshot_extras(run_id: &str, folders: &[(PathBuf, bool)], level: PermissionLevel) -> Vec<(ShadowRepo, String)> {
    if level == PermissionLevel::Plan {
        return vec![];
    }
    folders
        .iter()
        .filter(|(_, w)| *w)
        .filter_map(|(p, _)| {
            let repo = ShadowRepo::open_or_init(p).ok()?;
            let tree = repo.create_snapshot(&format!("{run_id}-extra-base")).ok()?;
            crate::snapshot::record_run_extra(run_id, &repo, tree.clone());
            Some((repo, tree))
        })
        .collect()
}

/// 其他文件夹做完的快照和改动（并进 `into` 的路径写绝对路径）。给回每个文件夹这一轮改了的文件（相对那个文件夹，收尾提交用）。
pub(crate) fn finish_extras(run_id: &str, extras: &[(ShadowRepo, String)], into: &mut ChangeSetSummary) -> Vec<(PathBuf, Vec<String>)> {
    let mut rounds = Vec::new();
    for (repo, before) in extras {
        let Ok(after) = repo.create_snapshot(&format!("{run_id}-extra-after")) else { continue };
        crate::snapshot::record_extra_finish(run_id, &repo.workspace_path, after.clone());
        let Ok(cs) = repo.diff_changeset(before, &after) else { continue };
        rounds.push((repo.workspace_path.clone(), cs.files.iter().map(|f| f.path.clone()).collect()));
        for mut f in cs.files {
            f.path = repo.workspace_path.join(&f.path).to_string_lossy().into_owned();
            into.files.push(f);
        }
    }
    rounds
}

/// 收尾时要的：最后一段回答、回合怎么结束的。
#[derive(Default)]
struct Ending {
    text: String,
    error: Option<String>,
}

pub(crate) fn start_peck_run(
    run: PeckRun,
    provider: Arc<dyn ModelProvider>,
    out: impl Fn(&AgentEvent) + Send + Sync + 'static,
) -> Result<String, String> {
    let PeckRun { store, active_runs, session_id, prompt, who, permission, level, model_id, effort, cost_cap, env, worktree, team, team_notes, images, search, compact } = run;
    let label = model_label(&model_id);
    let claimed = crate::run::claim_run(crate::run::ClaimSpec {
        store: &store,
        active_runs: &active_runs,
        session_id: &session_id,
        prompt: &prompt,
        who: &who,
        permission: permission.clone(),
        level,
        worktree,
        agent_id: "peck",
        label: &label,
    })?;
    let (run_id, workspace, workspace_id, cwd) = (claimed.run_id.clone(), claimed.workspace.clone(), claimed.workspace_id.clone(), claimed.cwd.clone());
    let (settings, folders) = (claimed.settings.clone(), claimed.folders.clone());
    let env = with_project_env(env, &settings.env_pairs());
    // 占了号以后出错：记成失败、放开项目（不然一直占着）。
    let fail = |why: String| crate::run::abandon(&store, &active_runs, &run_id, why);
    let (history, allowed, mut refs, prior_changed, fork) = {
        let db = store.lock().map_err(|e| fail(e.to_string()))?;
        // 这个任务之前几次改过的文件（撤销了的不算）。
        let prior_changed: Vec<String> = db
            .runs(&session_id)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.id != run_id && r.undone_at.is_none())
            .flat_map(|r| r.files.into_iter().map(|f| f.path))
            .collect();
        (
            db.harness_conversation(&session_id).unwrap_or_default(),
            db.harness_allowed(&workspace_id).unwrap_or_default(),
            super::history::task_refs(&db, &workspace_id, &prompt),
            prior_changed,
            // 换了引擎接着发（v0.8）：别家这期间做的带过来。
            super::history::fork_context(&db, &session_id, "peck", &run_id),
        )
    };
    if compact.is_some() {
        refs.clear();
    } else if let Some(f) = &fork {
        refs.insert(0, f.text.clone());
    }
    let fork_note = fork.as_ref().filter(|_| compact.is_none()).map(|f| super::history::fork_notice(f, &label));
    let context = ContextConfig {
        project: ProjectLayer::load(&workspace, memories_of(&store, &workspace_id)).with_skills(&workspace, &settings),
        environment: true,
        prior_changed,
        refs,
        history: Some(super::history::probe(store.clone(), workspace_id.clone())),
        compact_at: 0.0,
        folders: folders.clone(),
        env_names: settings.env.iter().map(|v| v.name.clone()).collect(),
        worktree_of: worktree.then(|| workspace.clone()),
    };
    // MCP（v0.10 A2）：设置里的 + 项目 .mcp.json 里的；读不懂、钥匙串取不到的开头写一行。
    let (mcp_list, mut mcp_notes) = {
        let db = store.lock().map_err(|e| fail(e.to_string()))?;
        let ws = db.workspace(&workspace_id).map_err(|e| fail(e.to_string()))?;
        super::mcp_client::for_project(&db, &ws)
    };
    // 手动压缩不起 MCP 服务器（不调工具）。
    let mcp_list = if compact.is_some() { vec![] } else { super::mcp_client::fill_secrets(mcp_list, &mut mcp_notes) };
    if compact.is_some() {
        mcp_notes.clear();
    } else {
        mcp_notes.extend(team_notes);
    }
    // 插件的收尾 Hook（v1.1.0）：项目里装着带它的插件（Git Plan）才有；循环收尾时问一句，快照拍完以后提交。手动压缩没有。
    let stop_hook = if worktree || compact.is_some() { None } else { crate::project_plugins::stop_hook(&workspace) };
    let stop_ask = super::hooks::StopAsk::of(&stop_hook);
    // 快照回退、定时任务（v1.0.2）：这个任务是谁在干，设定时任务时默认照它。
    let extras = Arc::new(super::extras::Extras {
        store: store.clone(),
        workspace_id: workspace_id.clone(),
        session_id: session_id.clone(),
        run_id: run_id.clone(),
        setup: crate::schedule::Setup { who: who.clone(), model: None, effort: effort.clone() },
    });
    let config = LoopConfig {
        model_id: model_id.clone(),
        max_rounds: max_rounds(&store),
        rounds_probe: Some({
            let store = store.clone();
            Arc::new(move || max_rounds(&store))
        }),
        budget: budget(&store, cost_cap),
        live_spend: Some(live_spend(&store)),
        oneshot: false,
        allowed,
        spill_dir: spill_root().join(&session_id),
        runtime_dir: runtime_dir(&workspace),
        env: env.clone(),
        session_ref: Some(format!("peck-{session_id}")),
        context,
        team,
        brief: None,
        extras: Some(extras),
        stop_ask: stop_ask.clone(),
        task_level: None,
        mcp: mcp_list,
        mcp_seen: Some(super::mcp_client::seen_hook(store.clone(), workspace_id.clone())),
        subtask: None,
        search,
        hooks: Some(Arc::new(super::script_hooks::HookSet::load(&cwd))),
    };
    let hook_store = store.clone();
    let hook_ws = workspace_id.clone();
    let backend = PeckBackend::new(provider, config, history).with_images(images).on_allow_always(move |rule| {
        if let Ok(s) = hook_store.lock() {
            let _ = s.harness_allow(&hook_ws, rule);
        }
    });

    let (control_tx, control_rx) = mpsc::unbounded_channel();
    runner::register_control(&run_id, control_tx.clone());
    let id = run_id.clone();
    tokio::spawn(async move {
        let ending = Arc::new(Mutex::new(Ending::default()));
        let seen = ending.clone();
        let sink = EventSink::new(id.clone(), Some(claimed.step_id.clone()), "peck", Some(store.clone()), move |e: &AgentEvent| {
            if let Ok(mut end) = seen.lock() {
                match &e.body {
                    EventBody::MessageCompleted { text, .. } => end.text = text.clone(),
                    EventBody::TurnCompleted { error, .. } => end.error = error.clone(),
                    _ => {}
                }
            }
            out(e);
        });
        runner::register_sink(&id, &sink);
        if let Some(text) = fork_note {
            sink.emit(EventBody::Notice { level: crate::gateway::event::NoticeLevel::Info, text });
        }
        for text in mcp_notes {
            sink.emit(EventBody::Notice { level: crate::gateway::event::NoticeLevel::Warn, text });
        }
        let life = crate::run::Lifecycle::begin(&store, &active_runs, &claimed, "peck", &label, level, worktree);
        // 停止：`run_cancel` 改成发打断。
        let interrupt = control_tx.clone();
        let watcher = crate::run::watch_cancel(claimed.cancel_rx.clone(), move || {
            let _ = interrupt.send(Control::Interrupt);
        });
        let task = AgentTask {
            run_id: id.clone(),
            prompt,
            cwd: cwd.clone(),
            permission: level,
            model: Some(model_id.clone()),
            effort,
            resume: None,
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: false,
            trusted: false,
        };
        let compacting = compact.is_some();
        let outcome = match compact {
            Some(focus) => backend.compact(task, sink.clone(), control_rx, focus).await,
            None => backend.run(task, sink.clone(), control_rx).await,
        };
        watcher.abort();
        runner::unregister_control(&id);
        runner::unregister_sink(&id);
        if let (Ok(conversation), Ok(db)) = (backend.conversation.lock(), store.lock()) {
            let _ = db.save_harness_conversation(&session_id, &model_id, &super::r#loop::without_images(&conversation));
        }

        let (end_text, end_error) = ending.lock().map(|e| (e.text.clone(), e.error.clone())).unwrap_or_default();
        let (step_status, run_status, summary) = match &outcome {
            Ok(o) => match o.reason {
                TurnEndReason::Done => (
                    StepStatus::Succeeded,
                    RunStatus::Succeeded,
                    if compacting {
                        "上下文压缩好了：前面的对话换成了一份交接摘要。".into()
                    } else if end_text.trim().is_empty() {
                        format!("{label} 做完了。")
                    } else {
                        end_text
                    },
                ),
                TurnEndReason::Interrupted => (StepStatus::Cancelled, RunStatus::Cancelled, "已停止。".into()),
                TurnEndReason::Limit => (StepStatus::Failed, RunStatus::Failed, end_error.unwrap_or_else(|| "到了上限，先停下。".into())),
                TurnEndReason::Refused => (StepStatus::Failed, RunStatus::Failed, "模型拒绝了这个请求。".into()),
                TurnEndReason::Error => (StepStatus::Failed, RunStatus::Failed, format!("没做完：{}", end_error.unwrap_or_else(|| "出错了。".into()))),
            },
            Err(e) => (StepStatus::Failed, RunStatus::Failed, format!("没做完：{}", e.message)),
        };
        let (rounds, changeset) = life.snapshot_after(&cwd);
        // 收尾 Hook（v1.1.0）：做完了、这一轮改到的仓库还有没提交的，用模型收尾时答的提交说明各提交一笔。
        let answered = stop_ask.clone();
        for body in super::hooks::finish(stop_hook.as_ref(), rounds, level, run_status == RunStatus::Succeeded, worktree, |_| async move { answered.and_then(|a| a.take()) }).await {
            sink.emit(body);
        }
        life.finish(step_status, run_status, summary, changeset);
    });
    Ok(run_id)
}

/// ONE SHOT：选一个引擎（API 模型或 CLI）只读回答一句。返回 id（`oneshot-…`）。
#[tauri::command]
pub async fn harness_oneshot(
    workspace_id: String,
    prompt: String,
    who: String,
    // CLI 引擎里的模型名（AGENTS 里选的；不填用 CLI 默认）。API 模型不看它。
    model: Option<String>,
    effort: Option<String>,
    cost_cap: Option<f64>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("先写一句话。".into());
    }
    let (workspace, settings) = {
        let db = state.store.lock().map_err(|_| "读不到项目。")?;
        let ws = db.workspace(&workspace_id).map_err(|e| e.to_string())?;
        (PathBuf::from(&ws.path), ws.settings)
    };
    if !workspace.is_dir() {
        return Err("项目文件夹不在了，重新打开一次。".into());
    }
    let id = format!("oneshot-{}", uuid::Uuid::new_v4().simple());
    let out = move |e: &AgentEvent| {
        let _ = channel.send(e.clone());
    };
    let cli_model = if who.starts_with("cli:") { model.filter(|m| !m.trim().is_empty()) } else { None };
    let (backend, engine): (Arc<dyn AgentBackend>, String) = if who.starts_with("api:") {
        let model_id = model_id_of(&who)?;
        let provider = providers::for_model(&state, &model_id).await.map_err(|e| e.message)?;
        let config = LoopConfig {
            model_id,
            max_rounds: oneshot_rounds(&state.store),
            rounds_probe: None,
            budget: budget(&state.store, cost_cap),
            live_spend: Some(live_spend(&state.store)),
            oneshot: true,
            allowed: HashSet::new(),
            spill_dir: spill_root().join(&id),
            runtime_dir: runtime_dir(&workspace),
            env: with_project_env(super::tools::bash::shell_env(), &settings.env_pairs()),
            session_ref: None,
            context: ContextConfig {
                project: ProjectLayer::load(&workspace, memories_of(&state.store, &workspace_id)).with_skills(&workspace, &settings),
                history: Some(super::history::probe(state.store.clone(), workspace_id.clone())),
                folders: settings.live_folders(),
                ..Default::default()
            },
            team: None,
            brief: None,
            extras: None,
            stop_ask: None,
            task_level: None,
            mcp: vec![],
            mcp_seen: None,
            subtask: None,
            search: crate::gateway::search::Keys::from_state(&state),
            hooks: None,
        };
        (Arc::new(PeckBackend::new(provider, config, vec![])), "peck".into())
    } else if let Some(engine) = who.strip_prefix("cli:") {
        (engines::from_state(&state, engine).await.map_err(|e| e.message)?, engine.to_string())
    } else {
        return Err("ONE SHOT 选一个 API 模型或 CLI 引擎。".into());
    };
    start_oneshot(state.store.clone(), id.clone(), engine, workspace, prompt, cli_model, effort, backend, out);
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn start_oneshot(
    store: Arc<Mutex<Store>>,
    id: String,
    engine: String,
    workspace: PathBuf,
    prompt: String,
    model: Option<String>,
    effort: Option<String>,
    backend: Arc<dyn AgentBackend>,
    out: impl Fn(&AgentEvent) + Send + Sync + 'static,
) {
    let (control_tx, control_rx) = mpsc::unbounded_channel();
    runner::register_control(&id, control_tx);
    tokio::spawn(async move {
        let sink = EventSink::new(id.clone(), None, engine, Some(store), out);
        let task = AgentTask {
            run_id: id.clone(),
            prompt,
            cwd: workspace,
            permission: PermissionLevel::Plan,
            model,
            effort,
            resume: None,
            peck_mcp: None,
            env: vec![],
            folders: vec![],
            compact: false,
            trusted: false,
        };
        let _ = backend.run(task, sink, control_rx).await;
        runner::unregister_control(&id);
    });
}

/// 打开落盘的完整命令输出（只认 Peck 输出目录里的文件，最多 4MB）。
#[tauri::command]
pub async fn harness_output_read(path: String) -> Result<String, String> {
    read_spill(&spill_root(), Path::new(&path))
}

/// 打开截图、附图（v0.8）：只认 Peck 输出目录里的图片，给 data URL（最多 12MB）。
#[tauri::command]
pub async fn harness_image_read(path: String) -> Result<String, String> {
    let root = spill_root().canonicalize().map_err(|_| "还没有存下来的图。".to_string())?;
    let file = Path::new(&path).canonicalize().map_err(|_| "这张图不在了。".to_string())?;
    if !file.starts_with(&root) {
        return Err("只能打开 Peck 存下来的图。".into());
    }
    let name = file.to_string_lossy().to_lowercase();
    if ![".png", ".jpg", ".jpeg", ".gif", ".webp"].iter().any(|e| name.ends_with(e)) {
        return Err("这不是图。".into());
    }
    let bytes = std::fs::read(&file).map_err(|_| "读不到这张图。".to_string())?;
    if bytes.len() > 12 * 1024 * 1024 {
        return Err("图太大了。".into());
    }
    Ok(crate::files::data_url(&name, &bytes))
}

/// 输入框 `+` 选到的是图（v1.1.11）：读成附图（和贴进来的一样发给模型），大的先缩到长边 2560。
#[tauri::command]
pub async fn upload_read_image(path: String) -> Result<crate::gateway::ImageInput, String> {
    tokio::task::spawn_blocking(move || read_picked_image(Path::new(&path))).await.map_err(|e| e.to_string())?
}

fn read_picked_image(path: &Path) -> Result<crate::gateway::ImageInput, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if !path.is_absolute() {
        return Err(format!("只收本机选框挑的绝对路径：{}", path.display()));
    }
    let lower = name.to_lowercase();
    let mut media = match lower.rsplit('.').next() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => return Err(format!("{name} 不是图。")),
    };
    let meta = std::fs::metadata(path).map_err(|_| format!("找不到了：{name}"))?;
    if meta.len() > 40 * 1024 * 1024 {
        return Err(format!("{name} 太大了。"));
    }
    let mut bytes = std::fs::read(path).map_err(|_| format!("读不到 {name}。"))?;
    // 贴图的上限是 6MB：大的（Retina 截图常见）缩一份再发，原图不动。
    if bytes.len() > 4 * 1024 * 1024 && cfg!(target_os = "macos") {
        let dir = spill_root().join("staging").join(uuid::Uuid::new_v4().simple().to_string());
        let _ = std::fs::create_dir_all(&dir);
        let small = dir.join("small.png");
        let ok = std::process::Command::new("/usr/bin/sips").args(["-Z", "2560", "-s", "format", "png"]).arg(path).arg("--out").arg(&small).output().is_ok_and(|o| o.status.success());
        if let Some(b) = ok.then(|| std::fs::read(&small).ok()).flatten() {
            bytes = b;
            media = "image/png";
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    crate::gateway::ImageInput { media_type: media.into(), data: crate::files::base64(&bytes) }.checked()
}

/// 拖进输入框的文档（v1.1.11）：浏览器给不了本机路径，先把内容存一份，回一个路径，和 `+` 附的一样发。
#[tauri::command]
pub async fn upload_stage(name: String, data: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || stage_upload(&spill_root(), &name, &data)).await.map_err(|e| e.to_string())?
}

fn stage_upload(root: &Path, name: &str, data: &str) -> Result<String, String> {
    let clean: String = name.chars().filter(|c| !matches!(c, '/' | '\\' | '\0')).collect::<String>().trim().trim_start_matches('.').to_string();
    let clean = if clean.is_empty() { "文档".to_string() } else { clean };
    let bytes = crate::files::base64_decode(data).ok_or("读不出这个文件。")?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(format!("{clean} 超过 10MB，拆小一点再附。"));
    }
    let dir = root.join("staging").join(uuid::Uuid::new_v4().simple().to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("放不下：{e}"))?;
    let path = dir.join(&clean);
    std::fs::write(&path, bytes).map_err(|e| format!("放不下：{e}"))?;
    Ok(path.to_string_lossy().into_owned())
}

fn read_spill(root: &Path, path: &Path) -> Result<String, String> {
    let root = root.canonicalize().map_err(|_| "还没有落盘的输出。".to_string())?;
    let file = path.canonicalize().map_err(|_| "这份输出不在了。".to_string())?;
    if !file.starts_with(&root) {
        return Err("只能打开 Peck 存下来的命令输出。".into());
    }
    let bytes = std::fs::read(&file).map_err(|_| "读不到这份输出。".to_string())?;
    let limit = 4 * 1024 * 1024;
    let mut text = String::from_utf8_lossy(&bytes[..bytes.len().min(limit)]).into_owned();
    if bytes.len() > limit {
        text.push_str(&format!("\n[只显示了前 4 MB，共 {} MB]", bytes.len() / 1024 / 1024));
    }
    Ok(text)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spend {
    /// 这个月 Peck Loop（API）花了多少美元。
    pub month_usd: f64,
    /// 这个月从哪天算起（毫秒）。
    pub since: i64,
}

/// `/compact`（v0.10 C2）：把这个任务存着的 Peck Loop 对话压一压，不叫模型、不花钱——长的工具输出落盘、
/// 最近两轮以外的工具输出换成一句「做过什么」、旧的思考和环境块去掉（自动压缩的前两步）。下一句发出去就小了。
/// `/compact [重点]`（v1.1.7）：Peck Loop 的手动压缩。落成这个任务的一次新记录（事件、花费、停止都照常），
/// 三步都做：落盘、清旧输出、叫一次模型写交接摘要（`focus` 接在要求后面）。用这个对话存的模型；它现在用不了就报原因，不换。
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn harness_compact(
    session_id: String,
    focus: Option<String>,
    permission: Option<String>,
    effort: Option<String>,
    cost_cap: Option<f64>,
    channel: Channel<AgentEvent>,
    state: tauri::State<'_, AppState>,
) -> Result<String, String> {
    let (model_id, empty) = {
        let db = state.store.lock().map_err(|e| e.to_string())?;
        (db.harness_conversation_model(&session_id).map_err(|e| e.to_string())?, db.harness_conversation(&session_id).map_err(|e| e.to_string())?.is_empty())
    };
    if empty || model_id.is_empty() {
        return Err("这个任务还没有 Peck Loop 的对话（CLI 的会话在 CLI 那边压）。".into());
    }
    let level = permission_of(permission.as_deref())?;
    catalog::catalog().model(&model_id).ok_or_else(|| format!("这个对话用的 {model_id} 已经不在模型目录里了，压不了。"))?;
    let provider = providers::for_model(&state, &model_id).await.map_err(|e| format!("这个对话用的 {} 现在用不了：{}", model_label(&model_id), e.message))?;
    let focus = focus.map(|f| f.trim().to_string()).filter(|f| !f.is_empty());
    let prompt = match &focus {
        Some(f) => format!("/compact {f}"),
        None => "/compact".into(),
    };
    start_peck_run(
        PeckRun {
            store: state.store.clone(),
            active_runs: state.active_runs.clone(),
            session_id,
            prompt,
            who: format!("api:{model_id}"),
            permission,
            level,
            model_id,
            effort,
            cost_cap,
            env: super::tools::bash::shell_env(),
            worktree: false,
            team: None,
            team_notes: vec![],
            images: vec![],
            search: crate::gateway::search::Keys::from_state(&state),
            compact: Some(focus),
        },
        provider,
        move |e: &AgentEvent| {
            let _ = channel.send(e.clone());
        },
    )
}

#[tauri::command]
pub async fn harness_spend(state: tauri::State<'_, AppState>) -> Result<Spend, String> {
    let since = month_start_ms(now_ms());
    let db = state.store.lock().map_err(|_| "读不到花费。")?;
    Ok(Spend { month_usd: db.harness_spend_since(since).map_err(|e| e.to_string())?, since })
}

/// 设置 → 权限：「总是允许」的一条。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllowRule {
    pub workspace_id: String,
    /// 项目文件夹（家目录写成 `~`）；项目已经不在列表里时为空。
    pub workspace: Option<String>,
    /// `install-deps`（装依赖）、`long-running`（起常驻进程）。
    pub rule: String,
    pub created_at: i64,
}

fn allow_rules(db: &crate::store::Store) -> Result<Vec<AllowRule>, String> {
    Ok(db
        .harness_allow_all()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(workspace_id, rule, created_at)| AllowRule {
            workspace: db
                .workspace(&workspace_id)
                .ok()
                .map(|w| crate::gateway::config::tilde(std::path::Path::new(&w.path))),
            workspace_id,
            rule,
            created_at,
        })
        .collect())
}

#[tauri::command]
pub fn harness_allow_list(state: tauri::State<'_, AppState>) -> Result<Vec<AllowRule>, String> {
    let db = state.store.lock().map_err(|_| "读不到设置。")?;
    allow_rules(&db)
}

/// 撤掉一条「总是允许」：下次再碰到又会问。
#[tauri::command]
pub fn harness_allow_remove(workspace_id: String, rule: String, state: tauri::State<'_, AppState>) -> Result<Vec<AllowRule>, String> {
    let db = state.store.lock().map_err(|_| "读不到设置。")?;
    db.harness_disallow(&workspace_id, &rule).map_err(|e| e.to_string())?;
    allow_rules(&db)
}

#[cfg(test)]
mod tests {
    use crate::flow::DispatchPlan;

    #[test]
    fn max_rounds_follow_the_setting_within_bounds() {
        let task = |saved| rounds_from(saved, "maxRounds", DEFAULT_MAX_ROUNDS);
        assert_eq!(task(None), DEFAULT_MAX_ROUNDS);
        assert_eq!(task(Some(r#"{"costCap":20}"#)), DEFAULT_MAX_ROUNDS);
        assert_eq!(task(Some(r#"{"maxRounds":80}"#)), 80);
        assert_eq!(task(Some(r#"{"maxRounds":1}"#)), 5);
        assert_eq!(task(Some(r#"{"maxRounds":1000}"#)), 1000);
        assert_eq!(task(Some(r#"{"maxRounds":9999}"#)), 1000);
        assert_eq!(task(Some("不是 JSON")), DEFAULT_MAX_ROUNDS);
        // ONE SHOT、子任务各读各的，没存用各自的默认（v1.0.3）。
        let saved = Some(r#"{"maxRounds":80,"oneshotRounds":30}"#);
        assert_eq!(rounds_from(saved, "oneshotRounds", DEFAULT_ONESHOT_ROUNDS), 30);
        // 不限（v1.1.12）：只管任务，ONE SHOT 照旧。
        assert_eq!(task_rounds_from(saved), 80);
        let unlimited = Some(r#"{"maxRounds":80,"roundsUnlimited":true,"oneshotRounds":30}"#);
        assert_eq!(task_rounds_from(unlimited), super::super::r#loop::UNLIMITED_ROUNDS);
        assert_eq!(rounds_from(unlimited, "oneshotRounds", DEFAULT_ONESHOT_ROUNDS), 30);
        assert_eq!(task_rounds_from(Some(r#"{"maxRounds":80,"roundsUnlimited":false}"#)), 80);
    }

    use super::*;
    use crate::gateway::event::{ApprovalDecision, Cost, CostSource, TokenUsage, UsageScope};
    use crate::gateway::mock::ScriptedModel;
    use std::time::Duration;

    #[test]
    fn uploads_copy_files_and_note_folders() {
        let src = std::env::temp_dir().join(format!("peck-uploads-src-{}", uuid::Uuid::new_v4()));
        let sub = src.join("参考资料");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(src.join("需求文档.md"), "# 需求\n").unwrap();
        let session = format!("upload-test-{}", uuid::Uuid::new_v4().simple());
        let saved = save_uploads(
            &session,
            &[src.join("需求文档.md").to_string_lossy().to_string(), sub.to_string_lossy().to_string()],
        )
        .unwrap();
        assert_eq!(saved.files.len(), 1);
        assert_eq!(saved.folders, vec![sub.clone()]);
        assert_eq!(std::fs::read_to_string(&saved.files[0]).unwrap(), "# 需求\n");
        let note = uploads_note(&saved);
        assert!(note.contains("1 个文档") && note.contains("1 个文件夹"));
        // 不收相对路径、链接和超大；根目录和家目录不收。
        assert!(save_uploads(&session, &["相对/路径.md".into()]).is_err());
        assert!(save_uploads(&session, &["/".into()]).is_err());
        let _ = std::fs::remove_dir_all(spill_root().join(&session));
        let _ = std::fs::remove_dir_all(src);
    }

    fn temp_store() -> (PathBuf, crate::store::Store, crate::workspace::Workspace) {
        let root = std::env::temp_dir().join(format!("peck-harness-run-{}", uuid::Uuid::new_v4()));
        let ws_dir = root.join("ws");
        std::fs::create_dir_all(ws_dir.join("src")).unwrap();
        std::fs::write(ws_dir.join("src/App.tsx"), "export default () => <h1>Todo</h1>;\n").unwrap();
        let store = Store::open(&root.join("peck.db")).unwrap();
        let ws = crate::workspace::Workspace {
            id: "w1".into(),
            path: ws_dir.to_string_lossy().into_owned(),
            name: "试".into(),
            last_opened_at: 0,
            git_branch: None,
            hidden: false,
            settings: Default::default(),
        };
        store.save_workspace(&ws).unwrap();
        (root, store, ws)
    }

    #[test]
    fn month_start_and_spend_count_only_this_month_of_peck() {
        // 2026-11-12 08:00 UTC → 2026-11-01 00:00 UTC
        assert_eq!(month_start_ms(1_794_441_600_000 + 8 * 3600 * 1000), 1_793_491_200_000);
        let (root, store, _) = temp_store();
        let usage = |usd: Option<f64>| EventBody::UsageUpdated {
            usage: TokenUsage::default(),
            cost: Cost { usd, source: if usd.is_some() { CostSource::Priced } else { CostSource::Unknown } },
            scope: UsageScope::Turn,
            model: None,
        };
        let store = Arc::new(Mutex::new(store));
        let peck = EventSink::new("r1", None, "peck", Some(store.clone()), |_| {});
        peck.emit(usage(Some(0.25)));
        peck.emit(usage(None));
        peck.emit(usage(Some(0.5)));
        EventSink::new("r2", None, "claude-code", Some(store.clone()), |_| {}).emit(usage(Some(9.0)));
        let s = store.lock().unwrap();
        assert!((s.harness_spend_since(0).unwrap() - 0.75).abs() < 1e-9);
        assert_eq!(s.harness_spend_since(now_ms() + 1000).unwrap(), 0.0);
        drop(s);
        assert_eq!(budget(&store, Some(0.0)), None);
        assert!((budget(&store, Some(5.0)).unwrap().spent_usd - 0.75).abs() < 1e-9);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn old_spill_folders_are_pruned() {
        let root = std::env::temp_dir().join(format!("peck-spill-{}", uuid::Uuid::new_v4()));
        let (fresh, stale) = (root.join("sess-new"), root.join("sess-old"));
        std::fs::create_dir_all(&fresh).unwrap();
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join("1-bash.log"), "x").unwrap();
        let month_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(30 * 86_400);
        std::fs::File::open(&stale).unwrap().set_modified(month_ago).unwrap();
        assert_eq!(prune_spills(&root, std::time::Duration::from_secs(14 * 86_400)), 1);
        assert!(fresh.exists() && !stale.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn conversations_and_always_allow_are_stored() {
        let (root, store, _) = temp_store();
        assert!(store.harness_conversation("s1").unwrap().is_empty());
        let msgs = vec![Message { role: crate::gateway::Role::User, parts: vec![crate::gateway::Part::Text { text: "你好".into() }] }];
        store.save_harness_conversation("s1", "deepseek/deepseek-flash", &msgs).unwrap();
        store.save_harness_conversation("s1", "deepseek/deepseek-flash", &msgs).unwrap();
        assert_eq!(store.harness_conversation("s1").unwrap(), msgs);
        store.harness_allow("w1", "install-deps").unwrap();
        store.harness_allow("w1", "install-deps").unwrap();
        assert_eq!(store.harness_allowed("w1").unwrap(), HashSet::from(["install-deps".to_string()]));
        assert!(store.harness_allowed("w2").unwrap().is_empty());
        assert_eq!(store.harness_allow_all().unwrap().len(), 1);
        store.harness_disallow("w1", "install-deps").unwrap();
        assert!(store.harness_allowed("w1").unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn only_spilled_output_can_be_opened() {
        let root = std::env::temp_dir().join(format!("peck-spill-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("run-1")).unwrap();
        std::fs::write(root.join("run-1/1-bash.log"), "全部的输出").unwrap();
        assert_eq!(read_spill(&root, &root.join("run-1/1-bash.log")).unwrap(), "全部的输出");
        assert!(read_spill(&root, &root.join("run-1/../../etc")).is_err());
        assert!(read_spill(&root, Path::new("/etc/hosts")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn writable_other_folders_show_in_changes_and_undo_with_the_task() {
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let (root, store, mut ws) = temp_store();
        let lib = root.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("keep.txt"), "原来的\n").unwrap();
        ws.settings.folders = vec![crate::workspace::ProjectFolder { path: lib.canonicalize().unwrap().to_string_lossy().into_owned(), writable: true }];
        ws.settings.env = vec![crate::workspace::EnvVar { name: "PECK_TEST_FLAVOR".replace("PECK_", "PROJ_"), value: "mint".into() }];
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        // 快照登记表是整个进程共用的，别的测试会用 run-105：先占一个大号。
        let mut store = store;
        let filler = DispatchPlan { steps: vec![] };
        store.create_run(&session.id, "占号", 930_001, &filler).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active: crate::run::ActiveRuns = Arc::default();
        let lib_file = lib.canonicalize().unwrap().join("made.txt");
        let keep = lib.canonicalize().unwrap().join("keep.txt");
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![
                crate::gateway::mock::ScriptTurn {
                    reasoning: None,
                    text: None,
                    tool_calls: vec![
                        ("write_file".into(), serde_json::json!({"path": lib_file.to_string_lossy(), "content": "新的\n"})),
                        ("bash_run".into(), serde_json::json!({"command": format!("echo $PROJ_TEST_FLAVOR > {}", keep.display())})),
                    ],
                    usage: TokenUsage { input: Some(100), output: Some(10), cache_read: Some(0), cache_write: Some(0), ..Default::default() },
                },
            ],
            Duration::ZERO,
        ));
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let run_id = start_peck_run(
            PeckRun {
                store: store.clone(),
                active_runs: active.clone(),
                session_id: session.id.clone(),
                prompt: "改别的文件夹".into(),
                who: "api:deepseek/deepseek-flash".into(),
                permission: Some("auto".into()),
                level: PermissionLevel::Auto,
                model_id: "deepseek/deepseek-flash".into(),
                effort: None,
                cost_cap: None,
                env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
                worktree: false,
                team: None,
                team_notes: vec![],
                images: vec![],
                search: Default::default(),
                compact: None,
            },
            model,
            move |e| {
                let _ = tx.send(e.clone());
            },
        )
        .unwrap();
        while let Some(e) = rx.recv().await {
            if matches!(e.body, EventBody::TurnCompleted { .. }) {
                break;
            }
        }
        for _ in 0..200 {
            if active.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(std::fs::read_to_string(&keep).unwrap().trim(), "mint", "项目的环境变量带进了命令");
        let (record, snapshot) = {
            let db = store.lock().unwrap();
            (db.runs(&session.id).unwrap().into_iter().find(|r| r.id == run_id).unwrap(), db.snapshot(&run_id).unwrap().unwrap())
        };
        let paths: Vec<&str> = record.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&lib_file.to_string_lossy().as_ref()) && paths.contains(&keep.to_string_lossy().as_ref()), "{paths:?}");
        let diff = crate::snapshot::get_diff_for_run(&run_id, &keep.to_string_lossy()).unwrap();
        assert!(diff.contains("+mint"), "{diff}");
        crate::snapshot::undo_run(&snapshot).unwrap();
        assert!(!lib_file.exists());
        assert_eq!(std::fs::read_to_string(&keep).unwrap(), "原来的\n");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Git Plan 装在主文件夹（v1.2.1）：这一轮改到的主文件夹和其他文件夹各提交一笔，其他文件夹照它自己 CLAUDE.md 里的 Git 规则。
    #[tokio::test]
    async fn git_plan_commits_each_changed_folder_by_its_own_rules() {
        use crate::harness::git::testing as g;
        if crate::harness::tools::bash::nested_sandbox() {
            return;
        }
        let (root, store, mut ws) = temp_store();
        let main = PathBuf::from(&ws.path).canonicalize().unwrap();
        g::init(&main);
        g::install_git_plan(&main);
        let lib = root.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        let lib = lib.canonicalize().unwrap();
        g::init(&lib);
        std::fs::write(lib.join("CLAUDE.md"), "# Lib\n\n## Git\n\n提交说明写 `lib: <描述>`。\n").unwrap();
        g::commit_all(&lib, "v0.0.2: rules");
        ws.settings.folders = vec![crate::workspace::ProjectFolder { path: lib.to_string_lossy().into_owned(), writable: true }];
        store.save_workspace(&ws).unwrap();
        let session = store.create_session(&ws.id, None).unwrap();
        let mut store = store;
        store.create_run(&session.id, "占号", 930_101, &DispatchPlan { steps: vec![] }).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active: crate::run::ActiveRuns = Arc::default();
        let turn = |text: Option<&str>, tools: Vec<(String, serde_json::Value)>| crate::gateway::mock::ScriptTurn { reasoning: None, text: text.map(str::to_string), tool_calls: tools, usage: TokenUsage { input: Some(100), output: Some(10), cache_read: Some(0), cache_write: Some(0), ..Default::default() } };
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![
                turn(None, vec![("write_file".into(), serde_json::json!({"path": "app.txt", "content": "app\n"})), ("write_file".into(), serde_json::json!({"path": lib.join("util.txt").to_string_lossy(), "content": "util\n"}))]),
                turn(Some("两边都改好了。"), vec![]),
                turn(Some("[1]\nv1.0.1: add app\n\n[2]\nlib: add util"), vec![]),
            ],
            Duration::ZERO,
        ));
        let requests = model.requests.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let _ = start_peck_run(
            PeckRun {
                store: store.clone(),
                active_runs: active.clone(),
                session_id: session.id.clone(),
                prompt: "两边都改".into(),
                who: "api:deepseek/deepseek-flash".into(),
                permission: Some("auto".into()),
                level: PermissionLevel::Auto,
                model_id: "deepseek/deepseek-flash".into(),
                effort: None,
                cost_cap: None,
                env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
                worktree: false,
                team: None,
                team_notes: vec![],
                images: vec![],
                search: Default::default(),
                compact: None,
            },
            model,
            move |e| {
                let _ = tx.send(e.clone());
            },
        )
        .unwrap();
        let mut hooks = Vec::new();
        while let Some(e) = tokio::time::timeout(Duration::from_secs(20), rx.recv()).await.ok().flatten() {
            if let EventBody::HookRan { ok, text, .. } = &e.body {
                hooks.push((*ok, text.clone()));
            }
            if hooks.len() == 2 {
                break;
            }
        }
        let asked = requests.lock().unwrap().last().unwrap().messages.last().unwrap().clone();
        assert!(matches!(&asked.parts[0], crate::gateway::Part::Text { text } if text.contains("[2]") && text.contains("提交说明写 `lib: <描述>`")), "{asked:?}");
        assert!(hooks.iter().all(|(ok, _)| *ok), "{hooks:?}");
        assert_eq!(g::log(&main, "%s")[0], "v1.0.1: add app");
        assert_eq!(g::files_in_head(&main), vec!["app.txt"]);
        assert_eq!(g::log(&lib, "%s")[0], "lib: add util");
        assert_eq!(g::files_in_head(&lib), vec!["util.txt"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_worktree_task_leaves_the_main_folder_until_applied() {
        let (root, store, ws) = temp_store();
        let main = PathBuf::from(&ws.path).canonicalize().unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git").current_dir(&main).args(args)
                .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@e").env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@e")
                .output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "."]);
        git(&["commit", "-qm", "first"]);
        let session = store.create_session(&ws.id, None).unwrap();
        // 快照登记表是整个进程共用的，别的测试会用 run-105：先占一个大号。
        let mut store = store;
        let filler = DispatchPlan { steps: vec![] };
        store.create_run(&session.id, "占号", 940_001, &filler).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active: crate::run::ActiveRuns = Arc::default();
        let model = Arc::new(ScriptedModel::new(
            "deepseek/deepseek-flash",
            vec![crate::gateway::mock::ScriptTurn {
                reasoning: None,
                text: None,
                tool_calls: vec![("write_file".into(), serde_json::json!({"path": "src/New.tsx", "content": "export const n = 1;\n"}))],
                usage: TokenUsage { input: Some(100), output: Some(10), cache_read: Some(0), cache_write: Some(0), ..Default::default() },
            }],
            Duration::ZERO,
        ));
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let run_id = start_peck_run(
            PeckRun {
                store: store.clone(),
                active_runs: active.clone(),
                session_id: session.id.clone(),
                prompt: "在独立工作区里加个文件".into(),
                who: "api:deepseek/deepseek-flash".into(),
                permission: Some("auto".into()),
                level: PermissionLevel::Auto,
                model_id: "deepseek/deepseek-flash".into(),
                effort: None,
                cost_cap: None,
                env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
                worktree: true,
                team: None,
                team_notes: vec![],
                images: vec![],
                search: Default::default(),
                compact: None,
            },
            model,
            move |e| {
                let _ = tx.send(e.clone());
            },
        )
        .unwrap();
        while let Some(e) = rx.recv().await {
            if matches!(e.body, EventBody::TurnCompleted { .. }) {
                break;
            }
        }
        for _ in 0..200 {
            if active.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let (record, snapshot) = {
            let db = store.lock().unwrap();
            (db.run(&run_id).unwrap(), db.snapshot(&run_id).unwrap().unwrap())
        };
        let wt = PathBuf::from(&record.worktree.clone().unwrap().path);
        assert!(!main.join("src/New.tsx").exists(), "主文件夹不动");
        assert!(wt.join("src/New.tsx").is_file());
        assert!(record.files.iter().any(|f| f.path == "src/New.tsx" && f.kind == "create"), "改动记录照常：{:?}", record.files);
        let repo = crate::worktree::shadow_repo(&wt, &main).unwrap();
        let applied = crate::worktree::apply(&repo, &main, &snapshot.base_tree, &run_id, false).unwrap();
        assert_eq!(applied.applied, vec!["src/New.tsx".to_string()]);
        assert_eq!(std::fs::read_to_string(main.join("src/New.tsx")).unwrap(), "export const n = 1;\n");
        crate::worktree::remove(&main, &wt);
        assert!(!wt.exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn a_peck_task_runs_end_to_end_records_changes_and_resumes() {
        let (root, store, ws) = temp_store();
        let session = store.create_session(&ws.id, None).unwrap();
        let mut store = store;
        let filler = DispatchPlan { steps: vec![] };
        store.create_run(&session.id, "占号", 920_001, &filler).unwrap();
        let store = Arc::new(Mutex::new(store));
        let active: crate::run::ActiveRuns = Arc::default();
        let make = |prompt: &str| PeckRun {
            store: store.clone(),
            active_runs: active.clone(),
            session_id: session.id.clone(),
            prompt: prompt.into(),
            who: "api:deepseek/deepseek-flash".into(),
            permission: Some("auto".into()),
            level: PermissionLevel::Auto,
            model_id: "deepseek/deepseek-flash".into(),
            effort: None,
            cost_cap: Some(100.0),
            env: vec![("PATH".into(), std::env::var("PATH").unwrap_or_default())],
            worktree: false,
            team: None,
            team_notes: vec![],
            images: vec![],
            search: Default::default(),
            compact: None,
        };
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        let run_id = start_peck_run(make("把标题改成中文"), Arc::new(ScriptedModel::peck_demo(Duration::ZERO)), move |e| {
            let _ = tx.send(e.clone());
        })
        .unwrap();
        assert_eq!(run_id, "run-920002");
        assert!(start_peck_run(make("再来"), Arc::new(ScriptedModel::peck_demo(Duration::ZERO)), |_| {}).unwrap_err().contains("一次跑一个"));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let e = tokio::time::timeout_at(deadline, rx.recv()).await.expect("跑太久了").expect("事件断了");
            match &e.body {
                EventBody::UserInputRequested { request_id, .. } => {
                    crate::gateway::runner::gateway_control(run_id.clone(), Control::answer(request_id.clone(), "先不改"))
                        .await
                        .unwrap();
                }
                EventBody::ApprovalRequested { request_id, .. } => {
                    crate::gateway::runner::gateway_control(run_id.clone(), Control::Approve { request_id: request_id.clone(), decision: ApprovalDecision::AllowOnce })
                        .await
                        .unwrap();
                }
                EventBody::TurnCompleted { .. } => break,
                _ => {}
            }
        }
        for _ in 0..200 {
            if active.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(active.lock().unwrap().is_empty());
        {
            let db = store.lock().unwrap();
            let record = db.runs(&session.id).unwrap().into_iter().find(|r| r.id == run_id).unwrap();
            assert_eq!(record.status, RunStatus::Succeeded, "{:?}", record.summary);
            assert_eq!(record.who.as_deref(), Some("api:deepseek/deepseek-flash"));
            assert!(record.summary.as_deref().unwrap_or("").contains("待办"));
            assert!(record.files.iter().any(|f| f.path == "src/App.tsx"), "任务记录里有改动：{:?}", record.files);
            assert_eq!(db.run_engine(&run_id).unwrap().as_deref(), Some("peck"));
            assert!(db.agent_events(&run_id).unwrap().len() > 30);
            assert_eq!(db.harness_conversation(&session.id).unwrap().len(), 16);
            assert!(db.harness_spend_since(0).unwrap() > 0.0);
        }
        assert!(crate::gateway::runner::gateway_control(run_id, Control::Interrupt).await.is_err(), "结束了就收不到控制");

        // 接着发一句：带着之前的对话
        let model = Arc::new(ScriptedModel::new("deepseek/deepseek-flash", vec![], Duration::ZERO));
        let requests = model.requests.clone();
        let (tx, mut rx) = mpsc::unbounded_channel::<AgentEvent>();
        start_peck_run(make("副标题呢？"), model, move |e| {
            let _ = tx.send(e.clone());
        })
        .unwrap();
        while let Some(e) = rx.recv().await {
            if matches!(e.body, EventBody::TurnCompleted { .. }) {
                break;
            }
        }
        assert_eq!(requests.lock().unwrap()[0].messages.len(), 17);
        let _ = std::fs::remove_dir_all(root);
    }
}
