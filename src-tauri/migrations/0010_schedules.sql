-- 定时任务（v0.8 Phase 12）：到点了 Peck 替站主在这个项目里发一句话，做完的进左栏「待你处理」。
-- 设置整段存一列 JSON（怎么定时、谁来干、权限、发什么）；next_at 是下一次的时间（毫秒，关了为空）。
CREATE TABLE schedules (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    record TEXT NOT NULL,
    next_at INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX schedules_next ON schedules(next_at);
