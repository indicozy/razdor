//! One SQLite file, two connections. The writer (`call`, `setup`) is the one connection that writes;
//! the reader (`read`) is a read-only connection with its own lock, so a dashboard query, a GET or the
//! health check never waits for a write. WAL makes that safe: a reader sees the last commit and never
//! blocks a writer. Both run on the blocking pool; the VPS has two shared vCPUs.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OpenFlags};

use crate::http::ApiError;

type Shared = Arc<Mutex<Connection>>;

#[derive(Clone)]
pub struct Db {
    conn: Shared,
    reader: Shared,
    /// Counts every query that goes through `call`, `read` or `setup` (the tests check the hot path).
    queries: Arc<AtomicU64>,
}

impl Db {
    pub fn open(path: &Path, schema: &str) -> Result<Db, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;",
        )?;
        conn.execute_batch(schema)?;
        // Opened after the writer has made the file and its WAL, so the read-only connection finds them.
        let reader = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        reader.execute_batch("PRAGMA busy_timeout=5000;")?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn)),
            reader: Arc::new(Mutex::new(reader)),
            queries: Arc::new(AtomicU64::new(0)),
        })
    }

    /// Runs `f` on the blocking pool with the writer. An error of `f` is the request's answer.
    pub async fn call<T, F>(&self, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&mut Connection) -> Result<T, ApiError> + Send + 'static,
        T: Send + 'static,
    {
        self.queries.fetch_add(1, Ordering::Relaxed);
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut guard)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("database task: {e}")))?
    }

    /// Runs a read-only `f` on the blocking pool with the reader connection. Use it for SELECTs only:
    /// the connection cannot write, and it never waits for the writer.
    pub async fn read<T, F>(&self, f: F) -> Result<T, ApiError>
    where
        F: FnOnce(&Connection) -> Result<T, ApiError> + Send + 'static,
        T: Send + 'static,
    {
        self.queries.fetch_add(1, Ordering::Relaxed);
        let conn = self.reader.clone();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&guard)
        })
        .await
        .map_err(|e| ApiError::Internal(format!("database task: {e}")))?
    }

    /// Runs a start-up step (a migration) on the writer, before the service takes requests.
    pub fn setup<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> rusqlite::Result<T> {
        self.queries.fetch_add(1, Ordering::Relaxed);
        let guard = self.conn.lock().unwrap_or_else(|e| e.into_inner());
        f(&guard)
    }

    /// The number of queries run so far (through `call`, `read` or `setup`).
    pub fn queries(&self) -> u64 {
        self.queries.load(Ordering::Relaxed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-db-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a test dir");
        dir
    }

    #[tokio::test]
    async fn an_open_read_does_not_block_a_write() {
        let dir = scratch("block");
        let db = Db::open(&dir.join("t.sqlite"), "CREATE TABLE IF NOT EXISTS t (v INTEGER);").expect("opens");
        let (started_tx, started_rx) = mpsc::channel::<()>();
        let (go_tx, go_rx) = mpsc::channel::<()>();
        let reader = {
            let db = db.clone();
            tokio::spawn(async move {
                db.read(move |c| {
                    let n: i64 = c.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))?;
                    started_tx.send(()).expect("the test listens");
                    // Hold the reader (and its snapshot) until the write has committed.
                    go_rx.recv().expect("the test sends");
                    Ok(n)
                })
                .await
            })
        };
        tokio::task::spawn_blocking(move || started_rx.recv().expect("the reader starts")).await.expect("joins");
        let write = tokio::time::timeout(
            Duration::from_secs(5),
            db.call(|c| {
                c.execute("INSERT INTO t (v) VALUES (1)", [])?;
                Ok(())
            }),
        )
        .await;
        go_tx.send(()).expect("the reader is waiting");
        write.expect("the write did not wait for the open read").expect("the write commits");
        assert_eq!(reader.await.expect("joins").expect("reads"), 0, "the read keeps its snapshot");
        let n: i64 = db.read(|c| Ok(c.query_row("SELECT COUNT(*) FROM t", [], |r| r.get(0))?)).await.expect("reads");
        assert_eq!(n, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_reader_cannot_write() {
        let dir = scratch("ro");
        let db = Db::open(&dir.join("t.sqlite"), "CREATE TABLE IF NOT EXISTS t (v INTEGER);").expect("opens");
        let refused = db.read(|c| Ok(c.execute("INSERT INTO t (v) VALUES (2)", []).is_err())).await.expect("runs");
        assert!(refused, "a read connection refuses a write");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
