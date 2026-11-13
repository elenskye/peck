-- 上下文（v0.6 Phase 7）：站主批过的项目记忆。模型用 memory 工具提，任务结束时站主在卡片上批；
-- 每次发任务放进系统提示的项目层（和 AGENTS.md / CLAUDE.md 一起）。
CREATE TABLE memories (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    text TEXT NOT NULL,
    -- 哪个任务提的（任务号对应的 run id）；手工加的为空。
    source_run TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX memories_workspace ON memories(workspace_id, created_at);
