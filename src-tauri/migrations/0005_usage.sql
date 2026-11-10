CREATE TABLE usage_calls (
    source TEXT NOT NULL,
    request_id TEXT NOT NULL,
    project TEXT NOT NULL DEFAULT '',
    run_id TEXT,
    task_id TEXT,
    round INTEGER,
    mode TEXT NOT NULL DEFAULT 'real',
    provider TEXT NOT NULL DEFAULT '',
    model TEXT NOT NULL DEFAULT '',
    role TEXT NOT NULL DEFAULT '',
    started_at INTEGER NOT NULL DEFAULT 0,
    finished_at INTEGER,
    status TEXT NOT NULL DEFAULT '',
    budget_counted INTEGER NOT NULL DEFAULT 0,
    input_tokens INTEGER,
    output_tokens INTEGER,
    cache_read_tokens INTEGER,
    cache_creation_tokens INTEGER,
    reasoning_tokens INTEGER,
    currency TEXT,
    actual_cost REAL,
    estimated_cost REAL,
    price_version TEXT,
    local_id INTEGER NOT NULL DEFAULT 0,
    imported_at INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (source, request_id)
);
CREATE INDEX usage_calls_started ON usage_calls(started_at);
CREATE INDEX usage_calls_project ON usage_calls(project);
CREATE INDEX usage_calls_model ON usage_calls(model);
CREATE TABLE usage_snapshots (
    client TEXT PRIMARY KEY,
    account_id TEXT,
    payload TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE usage_balances (
    provider TEXT PRIMARY KEY,
    payload TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT 0
);
