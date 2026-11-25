use crate::store::{now_ms, StoreResult};
use rusqlite::Connection;
use std::path::Path;

pub const VERSION: i64 = 12;
const BASELINE: &str = include_str!("../migrations/0001_history.sql");
const API: &str = include_str!("../migrations/0002_api.sql");
const WORKER: &str = include_str!("../migrations/0003_worker.sql");
const SUPERVISOR: &str = include_str!("../migrations/0004_supervisor.sql");
const USAGE: &str = include_str!("../migrations/0005_usage.sql");
const SETTINGS: &str = include_str!("../migrations/0006_settings.sql");
const GATEWAY: &str = include_str!("../migrations/0007_gateway.sql");
const HARNESS: &str = include_str!("../migrations/0008_harness.sql");
const CONTEXT: &str = include_str!("../migrations/0009_context.sql");
const SCHEDULES: &str = include_str!("../migrations/0010_schedules.sql");
const ARCHIVE: &str = include_str!("../migrations/0011_archive.sql");
const PROMPTS: &str = include_str!("../migrations/0012_prompts.sql");

/// 每一版的迁移，按版本号排好。
const STEPS: [(i64, &str); 12] = [
    (1, BASELINE),
    (2, API),
    (3, WORKER),
    (4, SUPERVISOR),
    (5, USAGE),
    (6, SETTINGS),
    (7, GATEWAY),
    (8, HARNESS),
    (9, CONTEXT),
    (10, SCHEDULES),
    (11, ARCHIVE),
    (12, PROMPTS),
];

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
    for (version, sql) in STEPS {
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
        // v11：旧任务都没归档。
        assert_eq!(
            conn.query_row("SELECT archived_at FROM sessions WHERE id='s1'", [], |r| r.get::<_, Option<i64>>(0))
                .unwrap(),
            None
        );
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

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)", [name], |r| r.get(0))
            .unwrap()
    }

    fn backups(root: &Path) -> Vec<std::path::PathBuf> {
        std::fs::read_dir(root.join("backups"))
            .map(|d| d.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "db")).collect())
            .unwrap_or_default()
    }

    #[test]
    fn fresh_database_gets_the_prompt_table_without_a_backup() {
        let root = std::env::temp_dir().join(format!("peck-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("peck.db");
        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn, &path).unwrap();
        assert_eq!(conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), 12);
        assert!(table_exists(&conn, "prompts"));
        assert!(backups(&root).is_empty(), "空库不备份");
        let _ = std::fs::remove_dir_all(root);
    }

    /// v1.2.0：从 schema 11 升上来，先备份一份 `before-v12`（还是 11、没有提示词表），旧数据都在。
    #[test]
    fn schema_11_upgrades_to_12_after_a_backup() {
        let root = std::env::temp_dir().join(format!("peck-migration-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("peck.db");
        let mut conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL;").unwrap();
        for (version, sql) in STEPS.iter().take_while(|(v, _)| *v <= 11) {
            conn.execute_batch(sql).unwrap();
            conn.pragma_update(None, "user_version", version).unwrap();
        }
        assert!(!table_exists(&conn, "prompts"));
        conn.execute("INSERT INTO workspaces VALUES ('w1', '{}', 5)", []).unwrap();
        conn.execute("INSERT INTO sessions(id, workspace_id, title, created_at, updated_at) VALUES ('s1','w1','旧任务',1,2)", []).unwrap();
        conn.execute("INSERT INTO memories(id, workspace_id, text, created_at) VALUES ('m1','w1','旧记忆',3)", []).unwrap();
        migrate(&mut conn, &path).unwrap();

        assert_eq!(conn.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), 12);
        assert!(table_exists(&conn, "prompts"));
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM prompts", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
        assert_eq!(conn.query_row("SELECT title FROM sessions WHERE id='s1'", [], |r| r.get::<_, String>(0)).unwrap(), "旧任务");
        assert_eq!(conn.query_row("SELECT text FROM memories WHERE id='m1'", [], |r| r.get::<_, String>(0)).unwrap(), "旧记忆");

        let found = backups(&root);
        assert_eq!(found.len(), 1);
        assert!(found[0].file_name().unwrap().to_string_lossy().ends_with("-before-v12.db"), "{:?}", found[0]);
        let backup = Connection::open(&found[0]).unwrap();
        assert_eq!(backup.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(), 11);
        assert!(!table_exists(&backup, "prompts"));
        assert_eq!(backup.query_row("SELECT text FROM memories", [], |r| r.get::<_, String>(0)).unwrap(), "旧记忆");
        drop(backup);

        // 已经是 12 了：再开不再备份、不重跑。
        migrate(&mut conn, &path).unwrap();
        assert_eq!(backups(&root).len(), 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(root);
    }
}
