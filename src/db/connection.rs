use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;
use turso::{Builder, Connection, Database};

/// Embedded migration SQL — single source of truth for the schema.
const MIGRATION_001: &str = include_str!("migrations/001_initial.sql");

/// The application database handle.
///
/// `Database` is `Clone` — background tasks can clone it and call `connect()`
/// to get their own `Connection` without re-opening the file.
#[derive(Clone)]
pub struct Db {
    pub conn: Connection,
    pub db: Database,
}

impl Db {
    /// Open (or create) the database at `path` and run migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path_str = path
            .as_ref()
            .to_str()
            .context("database path is not valid UTF-8")?
            .to_owned();

        // Ensure the parent directory exists.
        if let Some(parent) = path.as_ref().parent() {
            if !parent.as_os_str().is_empty() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("creating database directory {:?}", parent))?;
            }
        }

        info!(path = %path_str, "opening database");

        let db = Builder::new_local(&path_str)
            .build()
            .await
            .context("failed to open Turso database")?;

        let conn = db.connect().context("failed to get database connection")?;

        let mut handle = Self { conn, db };
        handle.migrate().await?;

        Ok(handle)
    }

    /// Run all embedded migrations.
    ///
    /// Uses `execute_batch` which handles multi-statement SQL correctly.
    /// All statements use `CREATE TABLE IF NOT EXISTS` / `CREATE INDEX IF NOT EXISTS`
    /// so re-running on startup is safe.
    async fn migrate(&mut self) -> Result<()> {
        info!("running database migrations");
        self.conn
            .execute_batch(MIGRATION_001)
            .await
            .context("migration 001 failed")?;
        info!("migrations complete");
        Ok(())
    }

    /// Spawn a fresh connection from the same underlying database.
    /// Used by background classification tasks.
    #[allow(dead_code)]
    pub fn new_connection(&self) -> Result<Connection> {
        self.db.connect().context("failed to create new connection")
    }
}
