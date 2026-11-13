use crate::store::{now_ms, StoreResult};
use rusqlite::Connection;
use std::path::Path;

pub const VERSION: i64 = 8;
const BASELINE: &str = include_str!("../migrations/0001_history.sql");
const API: &str = include_str!("../migrations/0002_api.sql");
const WORKER: &str = include_str!("../migrations/0003_worker.sql");
const SUPERVISOR: &str = include_str!("../migrations/0004_supervisor.sql");
const USAGE: &str = include_str!("../migrations/0005_usage.sql");
const SETTINGS: &str = include_str!("../migrations/0006_settings.sql");
const GATEWAY: &str = include_str!("../migrations/0007_gateway.sql");
const HARNESS: &str = include_str!("../migrations/0008_harness.sql");

pub fn migrate(conn: &mut Connection, path: &Path) -> StoreResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if current > VERSION {
        return Err("数据库来自较新的 Peck，请使用新版应用打开。".into());
    }
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;")?;
    let has_data: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%')",
        [], |r| r.get(0),
    )?;
    if current < VERSION && has_data {
        let dir = path.parent().unwrap_or(Path::new(".")).join("backups");
        std::fs::create_dir_all(&dir)?;
        let target = dir.join(format!("peck-{}-before-v{}.db", now_ms(), VERSION));
        // SQLite's backup API includes committed WAL contents; copying the .db alone does not.
        let mut backup = Connection::open(target)?;
        rusqlite::backup::Backup::new(conn, &mut backup)?.run_to_completion(
            100,
            std::time::Duration::from_millis(5),
            None,
        )?;
    }
    for (version, sql) in [
        (1, BASELINE),
        (2, API),
        (3, WORKER),
        (4, SUPERVISOR),
        (5, USAGE),
        (6, SETTINGS),
        (7, GATEWAY),
        (8, HARNESS),
    ] {
        if version <= current {
            continue;
        }
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", version)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unversioned_history_and_wal_survive_upgrade_and_backup() {
        let root = std::env::temp_dir().join(format!("peck-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("peck.db");
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        conn.execute_batch(BASELINE).unwrap();
        conn.execute("INSERT INTO workspaces VALUES ('old', '{}', 17)", [])
            .unwrap();
        conn.execute("INSERT INTO sessions VALUES ('s1','old','旧会话',1,2)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO runs VALUES ('r1',1,'s1','原运行','原计划','原快照索引')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO steps VALUES ('step1','r1',0,'旧步骤')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO messages VALUES ('m1','s1','user','旧消息','r1',1)",
            [],
        )
        .unwrap();
        migrate(&mut conn, &path).unwrap();
        assert_eq!(
            conn.query_row("SELECT snapshot FROM runs WHERE id='r1'", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "原快照索引"
        );
        assert_eq!(
            conn.query_row("SELECT content FROM messages WHERE id='m1'", [], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
            "旧消息"
        );
        assert_eq!(
            conn.query_row("SELECT last_opened_at FROM workspaces", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            17
        );
        let backup_path = std::fs::read_dir(root.join("backups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let backup = Connection::open(backup_path).unwrap();
        assert_eq!(
            backup
                .query_row("SELECT COUNT(*) FROM workspaces", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        migrate(&mut conn, &path).unwrap();
        drop(backup);
        assert_eq!(
            std::fs::read_dir(root.join("backups"))
                .unwrap()
                .filter_map(Result::ok)
                .filter(|e| e.path().extension().is_some_and(|x| x == "db"))
                .count(),
            1
        );
        conn.pragma_update(None, "user_version", VERSION + 1)
            .unwrap();
        assert!(migrate(&mut conn, &path).is_err());
    }
}
