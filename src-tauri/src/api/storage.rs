use super::profiles::Config;
use crate::store::{Store, StoreResult};
use rusqlite::{params, OptionalExtension};

impl Store {
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

}
