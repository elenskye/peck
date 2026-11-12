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
    /// `quota.updated` 顺手写进顶栏 QUOTA 的额度卡片（用非默认账号跑时关掉，免得两个账号的额度混在一起）。
    quota_sync: std::sync::atomic::AtomicBool,
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
                quota_sync: std::sync::atomic::AtomicBool::new(true),
            }),
            subagent_id: None,
        }
    }

    /// 收进内存的 sink（测试、夹具回放）。
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

    /// 这次用的不是 CLI 的默认账号：引擎报的额度不写进顶栏 QUOTA 的卡片（卡片只认默认账号）。
    pub fn without_quota_sync(self) -> Self {
        self.inner.quota_sync.store(false, Ordering::SeqCst);
        self
    }

    pub fn emit(&self, body: EventBody) -> AgentEvent {
        self.emit_raw(body, None)
    }

    /// 带原件发：原件只进日志，不发给界面。
    pub fn emit_raw(&self, body: EventBody, raw: Option<Value>) -> AgentEvent {
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
                        &self.inner.engine,
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
            if let EventBody::QuotaUpdated { provider, windows } = &event.body {
                if self.inner.quota_sync.load(Ordering::SeqCst) {
                    if let Ok(s) = store.lock() {
                        let _ = s.usage_record_gateway_quota(provider, windows);
                    }
                }
            }
        }
        (self.inner.out)(&event);
        event
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
        for row in rows {
            let (seq, at, engine, kind, json) = row?;
            out.push(serde_json::from_str(&json).unwrap_or(AgentEvent {
                seq: seq as u64,
                at,
                subagent_id: None,
                body: EventBody::Raw {
                    source: engine,
                    kind: Some(kind),
                },
            }));
        }
        Ok(out)
    }

    /// 一个事件的原件（看不懂的事件、排查格式问题时用）。
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

    #[test]
    fn engine_quota_reaches_the_quota_cards_only_for_the_default_account() {
        use crate::gateway::event::QuotaWindow;
        let store = temp_store();
        let windows = vec![
            QuotaWindow { label: "5 小时".into(), used_percent: Some(6.0), resets_at: Some(1) },
            QuotaWindow { label: "每周".into(), used_percent: Some(2.0), resets_at: Some(2) },
        ];
        let other = EventSink::new("r-other", None, "muse", Some(store.clone()), |_| {}).without_quota_sync();
        other.emit(EventBody::QuotaUpdated { provider: "muse".into(), windows: windows.clone() });
        assert!(store.lock().unwrap().usage_snapshots().unwrap().get("muse-code").is_none(), "非默认账号不写");

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
