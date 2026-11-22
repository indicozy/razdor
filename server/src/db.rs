//! One SQLite file behind a mutex, run on the blocking pool. WAL, so the dashboard reads while
//! the service writes; the mutex keeps one writer at a time (the VPS has two shared vCPUs).

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::http::ApiError;

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path, schema: &str) -> Result<Db, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        conn.execute_batch(schema)?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)) })
    }

    /// Runs `f` on the blocking pool with the connection. An error of `f` is the request's answer.
    pub async fn call<T, F>(&self, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&mut Connection) -> Result<T, ApiError> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut guard)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("database task: {e}")))?
    }

    /// Runs a start-up step (a migration) on the connection, before the service takes requests.
    pub fn setup(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<()>) -> rusqlite::Result<()> {
        let guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }

    /// Folds the WAL into the file (at shutdown, so a copy of the file alone is whole).
    pub async fn checkpoint(&self) -> Result<(), ApiError> {
        self.call(|c| {
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
            Ok(())
        })
        .await
    }
}
