-- 界面设置（主题、模式、谁来干、权限、通知、开发者选项……）：一行一个键，值是 JSON。
CREATE TABLE app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0
);
