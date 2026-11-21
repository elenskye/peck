-- Peck 循环（v0.5 Phase 6）：一个任务（会话）里 Peck 循环的对话，下一句接着它发；
-- 和供应商无关的 Message / Part（src-tauri/src/gateway/mod.rs），思考的私有数据原样存着。
CREATE TABLE harness_conversations (
    session_id TEXT PRIMARY KEY,
    model TEXT NOT NULL,
    messages TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
-- 「这个项目总是允许」：EDIT 档装依赖、起常驻进程这两类（rule 是 install-deps、long-running）。
CREATE TABLE harness_allow (
    workspace_id TEXT NOT NULL,
    rule TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, rule)
);
-- 算这个月的 API 花费用（花费上限）。
CREATE INDEX agent_events_engine_type_at ON agent_events(engine, type, at);
