use rusqlite::{Connection, Error as RusqliteError, Transaction, TransactionBehavior};
use std::path::Path;
use std::thread::sleep;
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Sqlite(#[from] RusqliteError),

    #[error("Timeout acquiring database lock after multiple attempts")]
    LockTimeout,

    #[error("Corrupt index schema: {0}")]
    SchemaMismatch(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

/// A robust SQLite connection wrapper enforcing WAL mode, foreign keys, and immediate transactions.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens or creates an on-disk SQLite database at `path` configured with WAL mode.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        Self::configure(conn)
    }

    /// Opens an in-memory SQLite database (primarily for fast unit tests).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        Self::configure(conn)
    }

    /// Configures essential PRAGMAs with a retry loop to withstand simultaneous multi-process launch.
    fn configure(conn: Connection) -> Result<Self, StorageError> {
        // Set default busy timeout to 5 seconds
        conn.busy_timeout(Duration::from_millis(5000))?;

        // Two processes opening a fresh index at once (e.g. GUI shell and index service helper)
        // can trigger SQLITE_BUSY during the WAL transition. Retry up to 50 times with 100ms backoff.
        let mut tries = 0;
        loop {
            let res = conn.execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;",
            );

            match res {
                Ok(_) => break,
                Err(RusqliteError::SqliteFailure(err, _))
                    if err.code == rusqlite::ffi::ErrorCode::DatabaseBusy && tries < 50 =>
                {
                    tries += 1;
                    sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(StorageError::Sqlite(e)),
            }
        }

        Ok(Self { conn })
    }

    /// Read-only or mutable direct access to the underlying Rusqlite connection.
    #[inline]
    pub fn raw(&self) -> &Connection {
        &self.conn
    }

    #[inline]
    pub fn raw_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Executes arbitrary SQL script statements without returning rows.
    pub fn execute_batch(&self, sql: &str) -> Result<(), StorageError> {
        self.conn.execute_batch(sql)?;
        Ok(())
    }

    /// Executes a transaction with IMMEDIATE locking to prevent mid-transaction lock escalation deadlocks.
    pub fn transaction<T, F>(&mut self, f: F) -> Result<T, StorageError>
    where
        F: FnOnce(&Transaction) -> Result<T, StorageError>,
    {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = f(&tx)?;
        tx.commit()?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_connection_pragmas() {
        let db = Database::open_in_memory().expect("Must open in-memory db");

        // Verify foreign keys are enabled
        let fk: i64 = db
            .raw()
            .query_row("PRAGMA foreign_keys;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk, 1);

        // Verify transactions work cleanly with immediate locking
        let mut db = db;
        db.transaction(|tx| {
            tx.execute("CREATE TABLE test (id INT);", [])?;
            tx.execute("INSERT INTO test VALUES (42);", [])?;
            Ok(())
        })
        .unwrap();

        let val: i64 = db
            .raw()
            .query_row("SELECT id FROM test;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(val, 42);
    }
}
