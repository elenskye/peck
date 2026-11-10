ALTER TABLE api_tasks ADD COLUMN record TEXT;
CREATE INDEX api_tasks_run ON api_tasks(run_id);
CREATE INDEX api_calls_task ON api_calls(task_id);
