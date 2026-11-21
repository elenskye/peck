CREATE TABLE api_settings (id INTEGER PRIMARY KEY CHECK(id=1), config TEXT NOT NULL, mode TEXT);
CREATE TABLE api_tasks (
    id TEXT PRIMARY KEY, run_id TEXT REFERENCES runs(id),
    parent_id TEXT REFERENCES api_tasks(id), role TEXT NOT NULL, status TEXT NOT NULL);
CREATE TABLE api_calls (
    id TEXT PRIMARY KEY, task_id TEXT REFERENCES api_tasks(id),
    started_at INTEGER NOT NULL, record TEXT NOT NULL);
CREATE INDEX api_calls_time ON api_calls(started_at);
