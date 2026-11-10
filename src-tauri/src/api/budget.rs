use rusqlite::{params, Connection, TransactionBehavior};
use serde::Serialize;
use std::path::Path;

const GRANT: &str = "peck-v0.2-2026-11-06";
const LIMITS: [(&str, &str, u32); 2] = [
    ("anthropic", "claude-opus-5-5", 20),
    ("deepseek", "deepseek-flash", 100),
];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Allowance {
    pub provider_id: String,
    pub api_model: String,
    pub limit: u32,
    pub used: u32,
}

fn open(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| "无法创建调用预算目录。")?;
    }
    let conn = Connection::open(path).map_err(|_| "无法打开调用预算账本。")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|_| "调用预算账本忙。")?;
    conn.execute_batch("PRAGMA journal_mode=WAL;
        CREATE TABLE IF NOT EXISTS grants (grant_id TEXT NOT NULL, provider TEXT NOT NULL, model TEXT NOT NULL,
            cap INTEGER NOT NULL, used INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(grant_id,provider,model));
        CREATE TABLE IF NOT EXISTS attempts (id TEXT PRIMARY KEY, grant_id TEXT NOT NULL, provider TEXT NOT NULL,
            model TEXT NOT NULL, started_at INTEGER NOT NULL);")
        .map_err(|_|"无法初始化调用预算账本。")?;
    for (provider, model, cap) in LIMITS {
        conn.execute(
            "INSERT OR IGNORE INTO grants(grant_id,provider,model,cap) VALUES(?1,?2,?3,?4)",
            params![GRANT, provider, model, cap],
        )
        .map_err(|_| "无法读取授权预算。")?;
    }
    Ok(conn)
}

pub fn read(path: &Path) -> Result<Vec<Allowance>, String> {
    let conn = open(path)?;
    let mut stmt = conn
        .prepare("SELECT provider,model,cap,used FROM grants WHERE grant_id=?1 ORDER BY provider")
        .map_err(|_| "无法读取预算。")?;
    let rows = stmt
        .query_map([GRANT], |r| {
            Ok(Allowance {
                provider_id: r.get(0)?,
                api_model: r.get(1)?,
                limit: r.get(2)?,
                used: r.get(3)?,
            })
        })
        .map_err(|_| "无法读取预算。")?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|_| "无法读取预算。".into())
}

pub fn reserve(path: &Path, id: &str, provider: &str, model: &str) -> Result<(), String> {
    let mut conn = open(path)?;
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "预算账本忙，请稍后重试。")?;
    let changed=tx.execute("UPDATE grants SET used=used+1 WHERE grant_id=?1 AND provider=?2 AND model=?3 AND used<cap",params![GRANT,provider,model])
        .map_err(|_|"无法占用调用预算。")?;
    if changed != 1 {
        return Err("该模型没有剩余授权次数；未发送请求。".into());
    }
    tx.execute(
        "INSERT INTO attempts VALUES(?1,?2,?3,?4,?5)",
        params![id, GRANT, provider, model, crate::store::now_ms()],
    )
    .map_err(|_| "调用编号已使用或预算记录失败；未发送请求。")?;
    tx.commit()
        .map_err(|_| "调用预算保存失败；未发送请求。".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn concurrent_reservations_never_exceed_grant_or_reset_on_reopen() {
        let path = std::env::temp_dir().join(format!("peck-budget-{}.db", uuid::Uuid::new_v4()));
        read(&path).unwrap();
        let threads: Vec<_> = (0..28)
            .map(|i| {
                let path = path.clone();
                std::thread::spawn(move || {
                    reserve(
                        &path,
                        &format!("attempt-{i}"),
                        "anthropic",
                        "claude-opus-5-5",
                    )
                    .is_ok()
                })
            })
            .collect();
        let successes = threads
            .into_iter()
            .filter_map(|t| t.join().ok())
            .filter(|v| *v)
            .count();
        assert_eq!(successes, 20);
        assert_eq!(
            read(&path)
                .unwrap()
                .iter()
                .find(|a| a.provider_id == "anthropic")
                .unwrap()
                .used,
            20
        );
        assert!(reserve(&path, "alias", "another-provider", "claude-opus-5-5").is_err());
        assert!(reserve(&path, "unknown", "deepseek", "other-model").is_err());
        let existing: String = open(&path)
            .unwrap()
            .query_row("SELECT id FROM attempts LIMIT 1", [], |r| r.get(0))
            .unwrap();
        assert!(reserve(&path, &existing, "deepseek", "deepseek-flash").is_err());
        assert_eq!(
            read(&path)
                .unwrap()
                .iter()
                .find(|a| a.provider_id == "deepseek")
                .unwrap()
                .used,
            0
        );
    }
}
