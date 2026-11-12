-- 统一事件日志（v0.5 Model Gateway）：一个任务的每条事件一行，按序号排。
-- event 是统一后的事件（JSON，src-tauri/src/gateway/event.rs）；raw 是引擎或供应商的原件，不认识的也存。
CREATE TABLE agent_events (
    run_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    step_id TEXT,
    at INTEGER NOT NULL,
    engine TEXT NOT NULL,
    type TEXT NOT NULL,
    event TEXT NOT NULL,
    raw TEXT,
    PRIMARY KEY (run_id, seq)
);
-- 任务是谁做的（`api:…`、`cli:…`、`flow:…`）。旧任务为空，照旧按旧记录显示。
ALTER TABLE runs ADD COLUMN engine TEXT;
