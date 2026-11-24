//! 事件日志和 `EventSink`：引擎发出的每条事件编号、存进 `agent_events`（连同原件），再交给界面。

use super::event::{AgentEvent, EventBody};
use crate::store::{now_ms, Store, StoreResult};
use rusqlite::{params, OptionalExtension};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type Out = Box<dyn Fn(&AgentEvent) + Send + Sync>;

/// 引擎往外发事件的唯一出口。克隆便宜（共享同一个序号）。
#[derive(Clone)]
pub struct EventSink {
    inner: Arc<Inner>,
    subagent_id: Option<String>,
}

struct Inner {
    run_id: String,
    step_id: Option<String>,
    engine: String,
    seq: AtomicU64,
    store: Option<Arc<Mutex<Store>>>,
    out: Out,
    /// 存日志失败只报一次，事件照常发给界面。
    store_failed: std::sync::atomic::AtomicBool,
    /// CLI 这个会话用的模型（`session.started` 报的）：它不报花费时按价格表估（v1.1.9）。
    session_model: Mutex<Option<String>>,
}

impl EventSink {
    /// `store` 为空时不落日志（测试、ONE SHOT 之类不记的）。序号接着这个任务已有的往后编。
    pub fn new(
        run_id: impl Into<String>,
        step_id: Option<String>,
        engine: impl Into<String>,
        store: Option<Arc<Mutex<Store>>>,
        out: impl Fn(&AgentEvent) + Send + Sync + 'static,
    ) -> Self {
        let run_id = run_id.into();
        let next = store
            .as_ref()
            .and_then(|s| s.lock().ok().and_then(|s| s.next_agent_event_seq(&run_id).ok()))
            .unwrap_or(0);
        Self {
            inner: Arc::new(Inner {
                run_id,
                step_id,
                engine: engine.into(),
                seq: AtomicU64::new(next),
                store,
                out: Box::new(out),
                store_failed: Default::default(),
                session_model: Default::default(),
            }),
            subagent_id: None,
        }
    }

    /// 收进内存的 sink（测试、夹具回放）。
    #[cfg(test)]
    pub fn memory(engine: impl Into<String>) -> (Self, Arc<Mutex<Vec<AgentEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink_events = events.clone();
        let sink = Self::new("memory", None, engine, None, move |e: &AgentEvent| {
            if let Ok(mut v) = sink_events.lock() {
                v.push(e.clone());
            }
        });
        (sink, events)
    }

    /// 子任务里的事件用这个 sink 发：自动带上 `subagent_id`，序号和主线共用。
    /// 主线的那一个（不带子任务 id）。`subagent.started` / `completed` 自己就带子任务 id，从子代理里派的（member 派的
    /// explorer，v1.1.3）也要走主线发，不然 JSON 里有两个 `subagentId`。
    pub fn root(&self) -> Self {
        Self { inner: self.inner.clone(), subagent_id: None }
    }

    pub fn for_subagent(&self, subagent_id: impl Into<String>) -> Self {
        Self {
            inner: self.inner.clone(),
            subagent_id: Some(subagent_id.into()),
        }
    }

    pub fn run_id(&self) -> &str {
        &self.inner.run_id
    }

    pub fn engine(&self) -> &str {
        &self.inner.engine
    }

    pub fn emit(&self, body: EventBody) -> AgentEvent {
        self.emit_raw(body, None)
    }

    /// 带原件发：原件只进日志，不发给界面。
    pub fn emit_raw(&self, mut body: EventBody, raw: Option<Value>) -> AgentEvent {
        // CLI 的用量没带花费（Muse、Codex 不报）：按会话的模型查价格表折成 API 价，标「估」（v1.1.9）。
        if self.subagent_id.is_none() && self.engine() != "peck" {
            if let Ok(mut model) = self.inner.session_model.lock() {
                estimate_cli_cost(&mut body, &mut model, now_ms());
            }
        }
        let event = AgentEvent {
            seq: self.inner.seq.fetch_add(1, Ordering::SeqCst),
            at: now_ms(),
            subagent_id: self.subagent_id.clone(),
            body,
        };
        if let Some(store) = &self.inner.store {
            let saved = store
                .lock()
                .map_err(|_| "事件日志锁坏了".to_string())
                .and_then(|s| {
                    s.append_agent_event(
                        &self.inner.run_id,
                        self.inner.step_id.as_deref(),
                        self.engine(),
                        &event,
                        raw.as_ref(),
                    )
                    .map_err(|e| e.to_string())
                });
            if let Err(error) = saved {
                if !self.inner.store_failed.swap(true, Ordering::SeqCst) {
                    eprintln!("Peck 事件日志写不进去（{}）：{error}", self.inner.run_id);
                }
            }
            // `quota.updated` 顺手写进顶栏 QUOTA 的额度卡片（CLI 只用和客户端同一个账号）。
            if let EventBody::QuotaUpdated { provider, windows } = &event.body {
                if let Ok(s) = store.lock() {
                    let _ = s.usage_record_gateway_quota(provider, windows);
                }
            }
        }
        (self.inner.out)(&event);
        event
    }
}

/// CLI 报的一条用量没有花费时，按这个会话的模型查价格表估一个（订阅不真花，标「估」）。
/// `model` 是到这一条为止会话报过的模型（`session.started`、换模型时更新）。Peck Loop 的用量自己带模型和花费，不碰。
pub(crate) fn estimate_cli_cost(body: &mut EventBody, model: &mut Option<String>, at: i64) {
    match body {
        EventBody::SessionStarted { model: Some(m), .. } => *model = Some(m.clone()),
        EventBody::SessionConfigChanged { model: Some(m), .. } => *model = Some(m.clone()),
        EventBody::UsageUpdated { usage, cost, model: None, .. } if cost.usd.is_none() => {
            if let Some(usd) = model.as_deref().and_then(|m| super::prices::book().usd_for(m, usage, false, at)) {
                *cost = super::event::Cost { usd: Some(usd), source: super::event::CostSource::Estimated };
            }
        }
        _ => {}
    }
}

impl Store {
    pub fn next_agent_event_seq(&self, run_id: &str) -> StoreResult<u64> {
        let max: Option<i64> = self
            .conn
            .query_row(
                "SELECT MAX(seq) FROM agent_events WHERE run_id = ?1",
                params![run_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(max.map(|m| m as u64 + 1).unwrap_or(0))
    }

    pub fn append_agent_event(
        &self,
        run_id: &str,
        step_id: Option<&str>,
        engine: &str,
        event: &AgentEvent,
        raw: Option<&Value>,
    ) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO agent_events(run_id, seq, step_id, at, engine, type, event, raw)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                run_id,
                event.seq as i64,
                step_id,
                event.at,
                engine,
                event.body.type_name(),
                serde_json::to_string(event)?,
                raw.map(|r| r.to_string()),
            ],
        )?;
        Ok(())
    }

    /// 一个任务的全部事件，按序号。读不懂的行（以后的版本写的）变成 `raw` 事件，不丢。
    pub fn agent_events(&self, run_id: &str) -> StoreResult<Vec<AgentEvent>> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, at, engine, type, event FROM agent_events WHERE run_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![run_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })?;
        let mut out = Vec::new();
        // 以前存下的 CLI 用量没有花费（v1.1.9 以前不估）：读的时候照现在的价格表补上，和新发的一样标「估」。
        let mut session_model: Option<String> = None;
        for row in rows {
            let (seq, at, engine, kind, json) = row?;
            let mut event = serde_json::from_str(&json).unwrap_or(AgentEvent {
                seq: seq as u64,
                at,
                subagent_id: None,
                body: EventBody::Raw {
                    source: engine.clone(),
                    kind: Some(kind),
                },
            });
            if engine != "peck" && event.subagent_id.is_none() {
                estimate_cli_cost(&mut event.body, &mut session_model, at);
            }
            out.push(event);
        }
        Ok(out)
    }

    /// 一个事件的原件（看不懂的事件、排查格式问题时用）。
    #[cfg(test)]
    pub fn agent_event_raw(&self, run_id: &str, seq: u64) -> StoreResult<Option<Value>> {
        let raw: Option<Option<String>> = self
            .conn
            .query_row(
                "SELECT raw FROM agent_events WHERE run_id = ?1 AND seq = ?2",
                params![run_id, seq as i64],
                |r| r.get(0),
            )
            .optional()?;
        Ok(raw.flatten().and_then(|s| serde_json::from_str(&s).ok()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gateway::event::{NoticeLevel, ToolKind};
    use serde_json::json;

    fn temp_store() -> Arc<Mutex<Store>> {
        let root = std::env::temp_dir().join(format!("peck-gw-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        Arc::new(Mutex::new(Store::open(&root.join("peck.db")).unwrap()))
    }

    /// CLI 不报花费（Muse、Codex）：按会话的模型估，存下的、发给界面的都带；以前存的没估过的，读的时候补上。
    #[test]
    fn cli_usage_without_a_cost_is_estimated_from_the_session_model() {
        use crate::gateway::event::{Cost, CostSource, TokenUsage, UsageScope};
        let usage = || EventBody::UsageUpdated {
            usage: TokenUsage { input: Some(1_000_000), output: Some(100_000), cache_read: Some(5_000_000), ..Default::default() },
            cost: Cost::unknown(),
            scope: UsageScope::Cumulative,
            model: None,
        };
        let started = |engine: &str| -> EventBody {
            serde_json::from_value(json!({"type": "session.started", "engine": engine, "model": "muse-spark-1.3", "caps": crate::gateway::caps::AgentCaps::muse(), "sessionRef": "s1"})).unwrap()
        };
        let store = temp_store();
        let sink = EventSink::new("run-cost", None, "muse", Some(store.clone()), |_| {});
        // 还不知道模型：照旧是未知。
        let early = sink.emit(usage());
        assert!(matches!(early.body, EventBody::UsageUpdated { cost: Cost { usd: None, .. }, .. }));
        sink.emit(started("muse"));
        let later = sink.emit(usage());
        let EventBody::UsageUpdated { cost, .. } = later.body else { panic!() };
        assert!(cost.usd.is_some_and(|v| v > 0.0) && cost.source == CostSource::Estimated);
        // 读回来：先前那条没估过的还是未知（那时没有模型），后面那条带着估的。
        let read = store.lock().unwrap().agent_events("run-cost").unwrap();
        assert!(matches!(&read[0].body, EventBody::UsageUpdated { cost: Cost { usd: None, .. }, .. }));
        assert!(matches!(&read[2].body, EventBody::UsageUpdated { cost: Cost { usd: Some(_), source: CostSource::Estimated }, .. }));
        // 以前的版本存下的（模型报过、花费是未知）：读的时候补上。
        {
            let db = store.lock().unwrap();
            for (seq, body) in [(0, started("muse")), (1, usage())] {
                db.append_agent_event("run-old", None, "muse", &AgentEvent { seq, at: 1, subagent_id: None, body }, None).unwrap();
            }
        }
        let old = store.lock().unwrap().agent_events("run-old").unwrap();
        assert!(matches!(&old[1].body, EventBody::UsageUpdated { cost: Cost { usd: Some(_), source: CostSource::Estimated }, .. }));
        // Peck Loop 的不碰（它自己按价格表算）。
        let (peck, seen) = EventSink::memory("peck");
        peck.emit(started("peck"));
        peck.emit(usage());
        assert!(matches!(&seen.lock().unwrap()[1].body, EventBody::UsageUpdated { cost: Cost { usd: None, .. }, .. }));
    }

    #[test]
    fn engine_quota_reaches_the_quota_cards() {
        use crate::gateway::event::QuotaWindow;
        let store = temp_store();
        let windows = vec![
            QuotaWindow { label: "5 小时".into(), used_percent: Some(6.0), resets_at: Some(1) },
            QuotaWindow { label: "每周".into(), used_percent: Some(2.0), resets_at: Some(2) },
        ];
        let sink = EventSink::new("r-default", None, "muse", Some(store.clone()), |_| {});
        sink.emit(EventBody::QuotaUpdated { provider: "muse".into(), windows });
        let cards = store.lock().unwrap().usage_snapshots().unwrap();
        let card = cards.get("muse-code").expect("Muse 的卡片");
        assert_eq!(card.status, "ok");
        assert_eq!(card.windows.len(), 2);
        assert_eq!(card.windows[0].window_duration_mins, Some(300));
        assert_eq!(card.windows[1].used_percent, Some(2.0));
    }

    #[test]
    fn sink_numbers_events_stores_raw_and_continues_after_reopen() {
        let store = temp_store();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        let sink = EventSink::new("run-1", Some("s1".into()), "codex", Some(store.clone()), move |e| {
            seen2.lock().unwrap().push(e.seq)
        });
        sink.emit(EventBody::TurnStarted { turn_id: "t1".into() });
        sink.for_subagent("sub-1").emit_raw(
            EventBody::ToolCallStarted {
                call_id: "c1".into(),
                name: "Read".into(),
                kind: ToolKind::Read,
                title: "读 a.txt".into(),
                input: json!({"path":"a.txt"}),
            },
            Some(json!({"method":"item/started"})),
        );
        assert_eq!(*seen.lock().unwrap(), vec![0, 1]);

        let again = EventSink::new("run-1", None, "codex", Some(store.clone()), |_| {});
        assert_eq!(
            again
                .emit(EventBody::Notice {
                    level: NoticeLevel::Info,
                    text: "接着编号".into()
                })
                .seq,
            2
        );

        let s = store.lock().unwrap();
        let events = s.agent_events("run-1").unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[1].subagent_id.as_deref(), Some("sub-1"));
        assert_eq!(s.agent_event_raw("run-1", 1).unwrap(), Some(json!({"method":"item/started"})));
        assert_eq!(s.agent_event_raw("run-1", 0).unwrap(), None);
        assert!(s.agent_events("run-2").unwrap().is_empty());
    }

    #[test]
    fn unreadable_rows_come_back_as_raw() {
        let store = temp_store();
        let s = store.lock().unwrap();
        s.conn
            .execute(
                "INSERT INTO agent_events(run_id, seq, at, engine, type, event) VALUES('r', 0, 5, 'grok', 'future.thing', '{\"type\":\"future.thing\"}')",
                [],
            )
            .unwrap();
        let events = s.agent_events("r").unwrap();
        assert_eq!(
            events[0].body,
            EventBody::Raw {
                source: "grok".into(),
                kind: Some("future.thing".into())
            }
        );
    }
}
