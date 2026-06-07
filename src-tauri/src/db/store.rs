//! Centralized SQL operations against the voxtrans SQLite pool.
//!
//! All persistence-side logic (CRUD on settings / tasks / segments / words /
//! terminology) lives here. The rest of the codebase calls into this module
//! rather than constructing SQL directly.

use sqlx::SqlitePool;

#[derive(Clone)]
pub struct TaskStore {
    pool: SqlitePool,
}

impl TaskStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[cfg(test)]
pub async fn test_pool() -> SqlitePool {
    // In-memory SQLite, no migrations needed for unit tests of pure conversion.
    // For tests that need full schema, use test_pool_with_migrations.
    sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(":memory:")
                .create_if_missing(true),
        )
        .await
        .expect("connect in-memory sqlite")
}

#[cfg(test)]
pub async fn test_pool_with_migrations() -> SqlitePool {
    let pool = test_pool().await;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .expect("enable FK");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("run migrations");
    pool
}
