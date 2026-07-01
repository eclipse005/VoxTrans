pub mod conversion;
pub mod models;
pub mod store;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use std::path::PathBuf;
use tauri::Manager;

/// Resolve the platform-specific default path of the GUI's SQLite database.
/// Used by `voxeval` so it reads the same data the Tauri app uses.
pub fn default_db_path() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA")
        .map_err(|e| format!("APPDATA env not set (Windows-only): {e}"))?;
    Ok(PathBuf::from(appdata)
        .join("com.voxtrans.desktop")
        .join("voxtrans.db"))
}

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

    // Hand-written idempotent ALTERs for columns added after the initial
    // schema. SQLite doesn't support `ADD COLUMN IF NOT EXISTS`, so we use
    // `PRAGMA table_info(settings)` to skip columns that already exist.
    for stmt in MIGRATION_ALTERS {
        let column_name = parse_settings_column_name(stmt)
            .ok_or_else(|| format!("migration statement does not target a settings column: {stmt}"))?;
        if has_settings_column(&pool, &column_name).await? {
            continue;
        }
        if let Err(e) = sqlx::query(stmt).execute(&pool).await {
            return Err(format!("failed to apply migration `{stmt}`: {e}"));
        }
    }

    Ok(pool)
}

/// Hand-written ALTER statements run after SCHEMA_SQL. Each is made idempotent
/// by the `has_settings_column` PRAGMA pre-check in `init_pool` (which skips a
/// statement if its target column already exists) — NOT by error suppression.
/// Add columns introduced after the initial schema here.
const MIGRATION_ALTERS: &[&str] = &[
    "ALTER TABLE settings ADD COLUMN enable_vision_assist INTEGER NOT NULL DEFAULT 0",
];

/// Parse the column name from an `ALTER TABLE settings ADD COLUMN <name> ...`
/// statement. Returns `None` if the statement doesn't match the expected shape.
fn parse_settings_column_name(sql: &str) -> Option<String> {
    let lower = sql.to_lowercase();
    let after_add_column = lower
        .split_once("add column")
        .map(|(_, rest)| rest.trim())?;
    let column_name = after_add_column
        .split_whitespace()
        .next()?;
    Some(column_name.to_string())
}

/// Check whether `settings` already has a column named `column_name`.
async fn has_settings_column(pool: &SqlitePool, column_name: &str) -> Result<bool, String> {
    // PRAGMA table_info returns (cid, name, type, notnull, dflt_value, pk).
    // We only need the `name` column at index 1.
    let rows = sqlx::query_as::<_, (i64, String, String, i64, Option<String>, i64)>(
        "PRAGMA table_info(settings)",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("failed to read settings schema: {e}"))?;
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
