//! One SQLite file, two connections. The writer (`call`, `setup`) is the one connection that writes;
//! the reader (`read`) is a read-only connection with its own lock, so a dashboard query, a GET or the
//! health check never waits for a write. WAL makes that safe: a reader sees the last commit and never
//! blocks a writer. Both run on the blocking pool; the VPS has two shared vCPUs.

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OpenFlags};
use tokio::sync::Semaphore;

use crate::http::ApiError;

type Shared = Arc<Mutex<Connection>>;

#[derive(Clone)]
pub struct Db {
    conn: Shared,
    reader: Shared,
    /// One permit per connection, taken in async before the blocking pool is used: a caller waiting
    /// for the connection holds no blocking thread, so a burst of writes cannot starve the pool.
    write_gate: Arc<Semaphore>,
    read_gate: Arc<Semaphore>,
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
            write_gate: Arc::new(Semaphore::new(1)),
            read_gate: Arc::new(Semaphore::new(1)),
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
        let permit = self.write_gate.clone().acquire_owned().await.map_err(|_| closed())?;
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
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
        let permit = self.read_gate.clone().acquire_owned().await.map_err(|_| closed())?;
        let conn = self.reader.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
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

fn closed() -> ApiError {
    ApiError::Internal("database closed".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn waiting_callers_hold_no_blocking_thread() {
        // Two blocking threads only. One writer holds its connection; ten more queue behind it. They
        // wait in async, so a blocking job of another kind still gets the second thread.
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .max_blocking_threads(2)
            .enable_all()
            .build()
            .expect("a runtime");
        let dir = scratch("pool");
        let db = Db::open(&dir.join("t.sqlite"), "CREATE TABLE IF NOT EXISTS t (v INTEGER);").expect("opens");
        rt.block_on(async {
            let (in_tx, in_rx) = mpsc::channel::<()>();
            let (hold_tx, hold_rx) = mpsc::channel::<()>();
            let first = {
                let db = db.clone();
                tokio::spawn(async move {
                    db.call(move |_c| {
                        in_tx.send(()).expect("the test listens");
                        hold_rx.recv().expect("the test sends");
                        Ok(())
                    })
                    .await
                })
            };
            tokio::task::spawn_blocking(move || in_rx.recv().expect("the writer starts")).await.expect("joins");
            let mut waiters = Vec::new();
            for _ in 0..10 {
                let db = db.clone();
                waiters.push(tokio::spawn(async move { db.call(|_c| Ok(())).await }));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            let quick = tokio::time::timeout(Duration::from_secs(2), tokio::task::spawn_blocking(|| 7)).await;
            assert_eq!(quick.expect("a blocking thread is free").expect("runs"), 7);
            hold_tx.send(()).expect("the writer is waiting");
            first.await.expect("joins").expect("the first write");
            for w in waiters {
                w.await.expect("joins").expect("the queued write");
            }
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

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
