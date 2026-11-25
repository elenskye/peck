-- 提示词库（v1.2.0 Phase 3）：全局一份、所有项目通用，右栏 PROMPT 里增删改查。
-- 正文照原样存（`{占位}` 不解析）；时间是毫秒，改字刷新 updated_at，列表按它新的在上。
CREATE TABLE prompts (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX prompts_updated ON prompts(updated_at);
