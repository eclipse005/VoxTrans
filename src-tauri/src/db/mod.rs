pub mod conversion;
pub mod models;
pub mod store;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::path::PathBuf;
use tauri::Manager;

/// Open a `SqlitePool` against `path` with the same options the GUI uses.
/// Skips the migration step because the GUI's process has already migrated.
pub async fn open_pool_at(path: &std::path::Path) -> Result<SqlitePool, String> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);
    SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .map_err(|e| format!("failed to open sqlite at {:?}: {e}", path))
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Idempotent schema bootstrap. `migrations/schema.sql` is plain SQL with
/// `CREATE TABLE/INDEX IF NOT EXISTS`, so it builds a fresh DB and is a no-op
/// on an existing one. We deliberately avoid `sqlx::migrate!`'s checksum
/// tracking: this is a local desktop app, schema changes go through hand-written
/// ALTERs or a DB rebuild, not a migration framework.
const SCHEMA_SQL: &str = include_str!("../../migrations/schema.sql");

pub async fn init_pool(app: &tauri::AppHandle) -> Result<SqlitePool, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;

    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("failed to create app data dir {:?}: {e}", app_data_dir))?;

    let db_path = app_data_dir.join("voxtrans.db");
    let options = connect_options(db_path)?;

    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(|e| format!("failed to connect sqlite: {e}"))?;

    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .map_err(|e| format!("failed to enable foreign keys: {e}"))?;

    sqlx::query(SCHEMA_SQL)
        .execute(&pool)
        .await
        .map_err(|e| format!("failed to apply schema: {e}"))?;

    apply_migration_alters(&pool).await?;

    Ok(pool)
}

/// Hand-written idempotent ALTERs for columns added after the initial schema.
///
/// SQLite doesn't support `ADD COLUMN IF NOT EXISTS`, so each entry is skipped
/// when `PRAGMA table_info(<table>)` already lists its column. Split out of
/// `init_pool` so an existing database can be migrated without a Tauri handle
/// (and so the legacy-schema path has a regression test).
pub(crate) async fn apply_migration_alters(pool: &SqlitePool) -> Result<(), String> {
    for mig in MIGRATION_ALTERS {
        if has_table_column(pool, mig.table, mig.column).await? {
            continue;
        }
        if let Err(e) = sqlx::query(mig.sql).execute(pool).await {
            return Err(format!("failed to apply migration `{}`: {e}", mig.sql));
        }
    }
    Ok(())
}

struct MigrationAlter {
    table: &'static str,
    column: &'static str,
    sql: &'static str,
}

/// Hand-written ALTER statements run after SCHEMA_SQL. Each is made idempotent
/// by the `has_table_column` PRAGMA pre-check in `init_pool` (which skips a
/// statement if its target column already exists) — NOT by error suppression.
/// Add columns introduced after the initial schema here.
const MIGRATION_ALTERS: &[MigrationAlter] = &[
    MigrationAlter {
        table: "settings",
        column: "enable_vision_assist",
        sql: "ALTER TABLE settings ADD COLUMN enable_vision_assist INTEGER NOT NULL DEFAULT 0",
    },
    MigrationAlter {
        table: "settings",
        column: "locale",
        sql: "ALTER TABLE settings ADD COLUMN locale TEXT NOT NULL DEFAULT 'zh-CN'",
    },
    MigrationAlter {
        table: "settings",
        column: "models_dir",
        sql: "ALTER TABLE settings ADD COLUMN models_dir TEXT",
    },
    MigrationAlter {
        table: "settings",
        column: "llm_profiles_json",
        sql: "ALTER TABLE settings ADD COLUMN llm_profiles_json TEXT NOT NULL DEFAULT '[]'",
    },
    MigrationAlter {
        table: "settings",
        column: "active_llm_profile_id",
        sql: "ALTER TABLE settings ADD COLUMN active_llm_profile_id TEXT NOT NULL DEFAULT 'deepseek'",
    },
    MigrationAlter {
        table: "settings",
        column: "default_review_source",
        sql: "ALTER TABLE settings ADD COLUMN default_review_source INTEGER NOT NULL DEFAULT 0",
    },
    MigrationAlter {
        table: "settings",
        column: "default_review_target",
        sql: "ALTER TABLE settings ADD COLUMN default_review_target INTEGER NOT NULL DEFAULT 0",
    },
    MigrationAlter {
        table: "tasks",
        column: "review_source",
        sql: "ALTER TABLE tasks ADD COLUMN review_source INTEGER NOT NULL DEFAULT 0",
    },
    MigrationAlter {
        table: "tasks",
        column: "review_target",
        sql: "ALTER TABLE tasks ADD COLUMN review_target INTEGER NOT NULL DEFAULT 0",
    },
    MigrationAlter {
        table: "tasks",
        column: "resume_from",
        sql: "ALTER TABLE tasks ADD COLUMN resume_from TEXT NOT NULL DEFAULT ''",
    },
    MigrationAlter {
        table: "settings",
        column: "enable_terminology_agent",
        sql: "ALTER TABLE settings ADD COLUMN enable_terminology_agent INTEGER NOT NULL DEFAULT 0",
    },
    MigrationAlter {
        table: "tasks",
        column: "enable_terminology_agent",
        sql: "ALTER TABLE tasks ADD COLUMN enable_terminology_agent INTEGER NOT NULL DEFAULT 0",
    },
    MigrationAlter {
        table: "subtitle_segments",
        column: "semantic_sentence_id",
        sql: "ALTER TABLE subtitle_segments ADD COLUMN semantic_sentence_id INTEGER NOT NULL DEFAULT 0",
    },
];

/// Check whether `table` already has a column named `column_name`.
async fn has_table_column(
    pool: &SqlitePool,
    table: &str,
    column_name: &str,
) -> Result<bool, String> {
    // Only allow known tables — PRAGMA table_name cannot be bound as a parameter.
    let pragma = match table {
        "settings" => "PRAGMA table_info(settings)",
        "tasks" => "PRAGMA table_info(tasks)",
        "subtitle_segments" => "PRAGMA table_info(subtitle_segments)",
        other => {
            return Err(format!("migration targets unsupported table: {other}"));
        }
    };
    // PRAGMA table_info returns (cid, name, type, notnull, dflt_value, pk).
    // We only need the `name` column at index 1.
    let rows = sqlx::query_as::<_, (i64, String, String, i64, Option<String>, i64)>(pragma)
        .fetch_all(pool)
        .await
        .map_err(|e| format!("failed to read {table} schema: {e}"))?;
    Ok(rows.into_iter().any(|row| row.1 == column_name))
}

fn connect_options(path: PathBuf) -> Result<SqliteConnectOptions, String> {
    if path.as_os_str().is_empty() {
        return Err("sqlite path is empty".to_string());
    }

    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upgrade path for databases created before `semantic_sentence_id`
    /// existed: the column must be added in place (defaulting to 0), and a
    /// re-run must be a no-op. Hydration rebuilds each task's JSON source of
    /// truth from these rows, so a missing column silently un-groups cues and
    /// degrades the two-stage translation back to per-cue units.
    #[tokio::test]
    async fn migration_adds_semantic_sentence_id_to_a_legacy_database() {
        let pool = crate::db::store::test_pool().await;
        // Pre-migration shape: the three tables, none of the ALTER columns.
        for ddl in [
            "CREATE TABLE settings (id INTEGER PRIMARY KEY)",
            "CREATE TABLE tasks (id TEXT PRIMARY KEY)",
            "CREATE TABLE subtitle_segments (\
             id TEXT PRIMARY KEY, task_id TEXT NOT NULL, idx INTEGER NOT NULL, \
             start_ms INTEGER NOT NULL, end_ms INTEGER NOT NULL, \
             source_text TEXT NOT NULL DEFAULT '', translated_text TEXT NOT NULL DEFAULT '', \
             updated_at INTEGER NOT NULL)",
        ] {
            sqlx::query(ddl)
                .execute(&pool)
                .await
                .expect("create legacy table");
        }
        sqlx::query(
            "INSERT INTO subtitle_segments \
             (id, task_id, idx, start_ms, end_ms, source_text, translated_text, updated_at) \
             VALUES ('s1', 't1', 0, 0, 1000, 'hi', '你好', 0)",
        )
        .execute(&pool)
        .await
        .expect("insert legacy row");

        apply_migration_alters(&pool)
            .await
            .expect("migrate a legacy database");
        // Idempotent: a second run must skip every column, not fail on it.
        apply_migration_alters(&pool)
            .await
            .expect("re-running the migration is a no-op");

        let sid: i64 = sqlx::query_scalar(
            "SELECT semantic_sentence_id FROM subtitle_segments WHERE id = 's1'",
        )
        .fetch_one(&pool)
        .await
        .expect("column exists after migration");
        assert_eq!(sid, 0, "legacy rows keep per-cue semantics");
    }
}
