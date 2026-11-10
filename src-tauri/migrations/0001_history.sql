-- Baseline for the shipped, unversioned v0.1 database. Do not alter shipped migrations.
CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY, data TEXT NOT NULL, last_opened_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspaces(id),
    title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS runs (
    id TEXT PRIMARY KEY, number INTEGER UNIQUE NOT NULL,
    session_id TEXT NOT NULL REFERENCES sessions(id), record TEXT NOT NULL,
    plan TEXT NOT NULL, snapshot TEXT);
CREATE TABLE IF NOT EXISTS steps (
    id TEXT PRIMARY KEY, run_id TEXT NOT NULL REFERENCES runs(id),
    step_index INTEGER NOT NULL, data TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY, session_id TEXT NOT NULL REFERENCES sessions(id),
    role TEXT NOT NULL, content TEXT NOT NULL, run_id TEXT REFERENCES runs(id),
    created_at INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS sessions_workspace ON sessions(workspace_id, updated_at);
CREATE INDEX IF NOT EXISTS runs_session ON runs(session_id, number);
CREATE INDEX IF NOT EXISTS messages_session ON messages(session_id, created_at);
