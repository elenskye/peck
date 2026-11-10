use super::{profiles::Config, ApiCall};
use crate::store::{Store, StoreResult};
use rusqlite::{params, OptionalExtension};

impl Store {
    pub fn insert_worker_task(&self, task: &super::worker::ApiTask) -> StoreResult<()> {
        self.conn.execute("INSERT INTO api_tasks(id,run_id,parent_id,role,status,record) VALUES(?1,?2,?3,?4,?5,?6)", params![task.id,task.run_id,task.parent_id,task.settings.role,task.status,serde_json::to_string(task)?])?;
        Ok(())
    }

    pub fn save_worker_task(&self, task: &super::worker::ApiTask) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE api_tasks SET status=?2,record=?3 WHERE id=?1",
            params![task.id, task.status, serde_json::to_string(task)?],
        )?;
        Ok(())
    }

    pub fn worker_task(&self, run_id: &str) -> StoreResult<Option<super::worker::ApiTask>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT record FROM api_tasks WHERE run_id=?1 AND parent_id IS NULL",
                [run_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        raw.map(|s| Ok(serde_json::from_str(&s)?)).transpose()
    }

    pub fn api_tasks(&self, run_id: &str) -> StoreResult<Vec<super::worker::ApiTask>> {
        let mut stmt = self.conn.prepare(
            "SELECT record FROM api_tasks WHERE run_id=?1 AND record IS NOT NULL ORDER BY rowid",
        )?;
        let rows = stmt
            .query_map([run_id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|raw| Ok(serde_json::from_str(&raw)?))
            .collect()
    }

    pub fn recover_worker_tasks(&self) -> StoreResult<()> {
        let mut stmt = self.conn.prepare(
            "SELECT record FROM api_tasks WHERE status='running' AND record IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for raw in rows {
            let mut task: super::worker::ApiTask = serde_json::from_str(&raw)?;
            task.status = "interrupted".into();
            task.finished_at = Some(crate::store::now_ms());
            task.summary=Some("应用退出时任务中断；不重放请求或工具。请检查当前文件和最后工具记录后发起继续任务。".into());
            for tool in &mut task.tools {
                if tool.status == "running" {
                    tool.status = "interrupted".into();
                    tool.finished_at = task.finished_at;
                    tool.output = "执行结果未知；可能已修改文件，不自动重放。".into();
                }
            }
            self.save_worker_task(&task)?;
        }
        Ok(())
    }

    // Deliberately excludes prompts, responses, reasoning, tool arguments and credentials.
    pub fn export_api_calls(&self) -> StoreResult<serde_json::Value> {
        let mut stmt=self.conn.prepare("SELECT c.record,w.data FROM api_calls c LEFT JOIN api_tasks t ON t.id=c.task_id LEFT JOIN runs r ON r.id=t.run_id LEFT JOIN sessions s ON s.id=r.session_id LEFT JOIN workspaces w ON w.id=s.workspace_id ORDER BY c.started_at,c.rowid")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut calls = vec![];
        for (raw, workspace) in rows {
            let c: ApiCall = serde_json::from_str(&raw)?;
            let project = workspace
                .and_then(|w| serde_json::from_str::<crate::workspace::Workspace>(&w).ok())
                .map(|w| w.name)
                .unwrap_or_else(|| "API 工作台".into());
            calls.push(serde_json::json!({"source":"peck","requestId":c.id,"project":project,"runId":c.run_id,"taskId":c.task_id,"round":c.round,"mode":c.mode,"provider":c.model.provider_id,"model":c.model.api_model,"role":c.role,"startedAt":c.started_at,"finishedAt":c.finished_at,"status":c.status,"budgetCounted":c.budget_counted,"usage":c.response.as_ref().map(|r|&r.usage),"currency":null,"actualCost":null,"estimatedCost":null,"priceVersion":null}));
        }
        Ok(
            serde_json::json!({"schemaVersion":1,"source":"peck","coverage":"Only requests recorded by this Peck installation; not account-wide usage or subscription quota","exportedAt":crate::store::now_ms(),"calls":calls}),
        )
    }

    pub fn api_config(&self) -> StoreResult<Config> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT config FROM api_settings WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        match raw {
            Some(raw) => Ok(serde_json::from_str(&raw)?),
            None => Ok(Config::default()),
        }
    }

    pub fn save_api_config(&self, config: &Config) -> StoreResult<()> {
        config.validate()?;
        self.conn.execute("INSERT INTO api_settings(id,config) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET config=excluded.config",[serde_json::to_string(config)?])?;
        Ok(())
    }

    pub fn api_mode(&self) -> StoreResult<Option<crate::mode::ApiMode>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT mode FROM api_settings WHERE id=1", [], |r| r.get(0))
            .optional()?
            .flatten();
        match raw.as_deref() {
            None => Ok(None),
            Some("mock") => Ok(Some(crate::mode::ApiMode::Mock)),
            Some("real") => Ok(Some(crate::mode::ApiMode::Real)),
            _ => Err("保存的 API 运行模式无效。".into()),
        }
    }

    pub fn save_api_mode(&self, mode: crate::mode::ApiMode) -> StoreResult<()> {
        self.conn.execute("INSERT INTO api_settings(id,config,mode) VALUES(1,?1,?2) ON CONFLICT(id) DO UPDATE SET mode=excluded.mode",
            params![serde_json::to_string(&Config::default())?,if mode==crate::mode::ApiMode::Mock {"mock"}else{"real"}])?;
        Ok(())
    }

    pub fn insert_api_call(&self, call: &ApiCall) -> StoreResult<()> {
        self.conn.execute(
            "INSERT INTO api_calls(id,task_id,started_at,record) VALUES(?1,?2,?3,?4)",
            params![
                call.id,
                call.task_id,
                call.started_at,
                serde_json::to_string(call)?
            ],
        )?;
        Ok(())
    }

    pub fn update_api_call(&self, call: &ApiCall) -> StoreResult<()> {
        self.conn.execute(
            "UPDATE api_calls SET record=?2 WHERE id=?1",
            params![call.id, serde_json::to_string(call)?],
        )?;
        Ok(())
    }

    pub fn api_calls(&self) -> StoreResult<Vec<ApiCall>> {
        let mut stmt = self
            .conn
            .prepare("SELECT record FROM api_calls ORDER BY started_at DESC,rowid DESC LIMIT 50")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }

    pub fn recover_api_calls(&self) -> StoreResult<()> {
        let mut stmt = self.conn.prepare("SELECT record FROM api_calls")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        for raw in rows {
            let mut call: ApiCall = serde_json::from_str(&raw)?;
            if call.status == "running" {
                call.status = "interrupted".into();
                call.error =
                    Some("应用退出时请求尚未完成；不会自动重发，已占用次数继续保留。".into());
                call.finished_at = Some(crate::store::now_ms());
                self.update_api_call(&call)?;
            }
        }
        Ok(())
    }
}
