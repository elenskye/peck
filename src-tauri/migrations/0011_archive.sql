-- 归档（v0.9 Phase 14，站主 2026-11-16 加）：任务收起来不在左栏列，记录全留着，设置 → ARCHIVE 里能看、拿回来、删掉。
-- 归档的时间（毫秒），没归档为空。
ALTER TABLE sessions ADD COLUMN archived_at INTEGER;
