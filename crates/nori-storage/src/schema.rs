use crate::connection::{Database, StorageError};
use rusqlite::Transaction;

/// Current database schema version.
pub const SCHEMA_VERSION: i64 = 5;

/// FTS5 column definitions and tokenizer configuration:
/// - `porter`: English stemming.
/// - `unicode61`: Standard Unicode casing and punctuation folding.
/// - `remove_diacritics 2`: Canonical diacritic stripping.
/// - `categories 'L* N* Co M*'`: Includes Mark (M*) characters so Bengali/Hindi vowel signs
///   and Arabic harakat stay attached to their words instead of splitting words apart.
pub const FTS_TOKENIZER: &str = "porter unicode61 remove_diacritics 2 categories 'L* N* Co M*'";

pub struct Schema;

impl Schema {
    /// Checks `PRAGMA user_version` and executes migrations up to `SCHEMA_VERSION` (5).
    pub fn migrate(db: &mut Database) -> Result<(), StorageError> {
        let version: i64 = db
            .raw()
            .query_row("PRAGMA user_version;", [], |row| row.get(0))?;

        if version > SCHEMA_VERSION {
            return Err(StorageError::SchemaMismatch(format!(
                "Database was created by a newer Nori version (schema {})",
                version
            )));
        }

        if version == SCHEMA_VERSION {
            return Ok(());
        }

        // Apply migrations inside an immediate transaction
        db.transaction(|tx| {
            let cur_version: i64 = tx.query_row("PRAGMA user_version;", [], |row| row.get(0))?;

            if cur_version == 0 {
                Self::create_v5(tx)?;
            } else {
                // Incremental migration steps for legacy databases
                if cur_version < 5 {
                    Self::migrate_to_v5(tx, cur_version)?;
                }
            }

            tx.execute(&format!("PRAGMA user_version = {};", SCHEMA_VERSION), [])?;
            Ok(())
        })?;

        Ok(())
    }

    /// Creates fresh Schema v5 tables from scratch.
    fn create_v5(tx: &Transaction) -> Result<(), StorageError> {
        tx.execute_batch(&format!(
            "CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS files (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                path TEXT NOT NULL UNIQUE,
                kind TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime REAL NOT NULL,
                inode INTEGER NOT NULL,
                device INTEGER NOT NULL,
                state TEXT NOT NULL DEFAULT 'pending',
                detail TEXT,
                info TEXT,
                indexed_at REAL
            );

            CREATE INDEX IF NOT EXISTS idx_files_inode ON files(device, inode);
            CREATE INDEX IF NOT EXISTS idx_files_state ON files(state, kind);

            CREATE TABLE IF NOT EXISTS segments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                file INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
                kind TEXT NOT NULL,
                modality TEXT NOT NULL,
                loc REAL,
                loc_end REAL,
                excerpt TEXT,
                vec BLOB
            );

            CREATE INDEX IF NOT EXISTS idx_segments_file ON segments(file);
            CREATE INDEX IF NOT EXISTS idx_segments_kind ON segments(kind);

            CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
                name,
                body,
                original UNINDEXED,
                tokenize = \"{}\"
            );",
            FTS_TOKENIZER
        ))?;

        Ok(())
    }

    /// Handles forward migrations from versions 1..4 up to 5.
    fn migrate_to_v5(tx: &Transaction, from_version: i64) -> Result<(), StorageError> {
        if from_version < 3 {
            // Nullify OCR segment vectors so screenshots are retrieved via vision embedding or FTS only
            tx.execute(
                "UPDATE segments SET vec = NULL, modality = 'none' WHERE kind = 'ocr';",
                [],
            )?;
        }

        if from_version < 5 {
            // Long file checkpoints in files table
            tx.execute(
                "UPDATE files SET state = 'partial' WHERE state = 'done' AND kind IN ('pdf', 'doc');",
                [],
            )?;
        }

        Ok(())
    }

    /// Retrieves a string value from table `meta`.
    pub fn get_meta(db: &Database, key: &str) -> Result<Option<String>, StorageError> {
        let mut stmt = db
            .raw()
            .prepare_cached("SELECT value FROM meta WHERE key = ?;")?;
        let mut rows = stmt.query([key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Upserts or removes a key in table `meta`.
    pub fn set_meta(db: &Database, key: &str, value: Option<&str>) -> Result<(), StorageError> {
        if let Some(val) = value {
            db.raw().execute(
                "INSERT INTO meta(key, value) VALUES(?, ?)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value;",
                [key, val],
            )?;
        } else {
            db.raw().execute("DELETE FROM meta WHERE key = ?;", [key])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_migration_fresh() {
        let mut db = Database::open_in_memory().unwrap();
        Schema::migrate(&mut db).expect("Migration should succeed");

        // Verify version set to 5
        let ver: i64 = db
            .raw()
            .query_row("PRAGMA user_version;", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ver, SCHEMA_VERSION);

        // Verify meta table upsert
        Schema::set_meta(&db, "fingerprint", Some("photo140 shot280")).unwrap();
        let val = Schema::get_meta(&db, "fingerprint").unwrap();
        assert_eq!(val, Some("photo140 shot280".to_string()));

        // Delete meta
        Schema::set_meta(&db, "fingerprint", None).unwrap();
        let val = Schema::get_meta(&db, "fingerprint").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn test_fts5_multilingual_tokenizer() {
        let mut db = Database::open_in_memory().unwrap();
        Schema::migrate(&mut db).unwrap();

        // Insert Bengali and English text into FTS
        db.raw()
            .execute(
                "INSERT INTO fts(rowid, name, body) VALUES(1, 'test.txt', 'কলকাতা একটি সুন্দর শহর');",
                [],
            )
            .unwrap();

        // Query Bengali word
        let count: i64 = db
            .raw()
            .query_row(
                "SELECT count(*) FROM fts WHERE fts MATCH 'কলকাতা';",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
