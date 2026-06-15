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

    Ok(pool)
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
