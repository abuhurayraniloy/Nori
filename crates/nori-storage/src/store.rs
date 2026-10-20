use crate::connection::{Database, StorageError};
use crate::schema::Schema;
use nori_core::{FileKind, FileRecord, Modality, SegmentKind, Vectors};
use nori_text::SearchText;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct IndexStore {
    db: Database,
}

impl IndexStore {
    /// Opens an index database at `path` and runs schema migrations.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let mut db = Database::open(path)?;
        Schema::migrate(&mut db)?;
        Ok(Self { db })
    }

    /// Opens an in-memory index database (ideal for tests).
    pub fn open_in_memory() -> Result<Self, StorageError> {
        let mut db = Database::open_in_memory()?;
        Schema::migrate(&mut db)?;
        Ok(Self { db })
    }

    #[inline]
    pub fn db(&self) -> &Database {
        &self.db
    }

    #[inline]
    pub fn db_mut(&mut self) -> &mut Database {
        &mut self.db
    }

    // MARK: - File Operations

    /// Registers a newly crawled file with state 'pending' and creates its initial FTS name segment.
    pub fn insert_file(
        &mut self,
        path: &str,
        kind: FileKind,
        size: i64,
        mtime: f64,
        inode: u64,
        device: u64,
    ) -> Result<i64, StorageError> {
        let file_id: i64 = self.db.transaction(|tx| {
            tx.execute(
                "INSERT INTO files(path, kind, size, mtime, inode, device, state)
                 VALUES(?, ?, ?, ?, ?, ?, 'pending');",
                rusqlite::params![
                    path,
                    kind.as_str(),
                    size,
                    mtime,
                    inode as i64,
                    device as i64
                ],
            )?;

            let id = tx.last_insert_rowid();

            // Insert initial name segment for FTS keyword matching
            tx.execute(
                "INSERT INTO segments(file, kind, modality) VALUES(?, 'name', 'none');",
                [id],
            )?;
            let seg_id = tx.last_insert_rowid();

            let file_name = Path::new(path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(path);
            let folder_text = Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            let norm_name = SearchText::normalized(file_name);
            let norm_folder = SearchText::normalized(&folder_text);

            tx.execute(
                "INSERT INTO fts(rowid, name, body) VALUES(?, ?, ?);",
                rusqlite::params![seg_id, norm_name, norm_folder],
            )?;

            Ok(id)
        })?;

        Ok(file_id)
    }

    /// Looks up a file record by filesystem inode and device number (for move/rename detection).
    pub fn find_file_by_inode(
        &self,
        device: u64,
        inode: u64,
    ) -> Result<Option<FileRecord>, StorageError> {
        let mut stmt = self.db.raw().prepare_cached(
            "SELECT id, path, kind, size, mtime, inode, device, state
             FROM files WHERE device = ? AND inode = ? LIMIT 1;",
        )?;

        let mut rows = stmt.query(rusqlite::params![device as i64, inode as i64])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::parse_file_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Looks up a file record by path.
    pub fn find_file_by_path(&self, path: &str) -> Result<Option<FileRecord>, StorageError> {
        let mut stmt = self.db.raw().prepare_cached(
            "SELECT id, path, kind, size, mtime, inode, device, state
             FROM files WHERE path = ? LIMIT 1;",
        )?;

        let mut rows = stmt.query([path])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Self::parse_file_row(row)?))
        } else {
            Ok(None)
        }
    }

    /// Updates file path when moved or renamed without re-indexing segments or vectors.
    pub fn move_file(&mut self, file_id: i64, new_path: &str) -> Result<(), StorageError> {
        self.db.transaction(|tx| {
            tx.execute(
                "UPDATE files SET path = ? WHERE id = ?;",
                rusqlite::params![new_path, file_id],
            )?;

            let file_name = Path::new(new_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(new_path);
            let folder_text = Path::new(new_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default();

            let norm_name = SearchText::normalized(file_name);
            let norm_folder = SearchText::normalized(&folder_text);

            tx.execute(
                "UPDATE fts SET name = ?, body = ?
                 WHERE rowid IN (SELECT id FROM segments WHERE file = ? AND kind = 'name');",
                rusqlite::params![norm_name, norm_folder, file_id],
            )?;

            Ok(())
        })?;

        Ok(())
    }

    /// Marks a file as completed or partial with metadata info.
    pub fn finish_file(
        &self,
        file_id: i64,
        state: &str,
        detail: Option<&str>,
        info: Option<&str>,
    ) -> Result<(), StorageError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);

        self.db.raw().execute(
            "UPDATE files SET state = ?, detail = ?, info = ?, indexed_at = ? WHERE id = ?;",
            rusqlite::params![state, detail, info, now, file_id],
        )?;

        Ok(())
    }

    /// Deletes a file and all associated segments and FTS index entries.
    pub fn delete_file(&mut self, file_id: i64) -> Result<(), StorageError> {
        self.db.transaction(|tx| {
            // Delete FTS entries for all segments of this file
            tx.execute(
                "DELETE FROM fts WHERE rowid IN (SELECT id FROM segments WHERE file = ?);",
                [file_id],
            )?;
            // Delete file record (cascades to segments table)
            tx.execute("DELETE FROM files WHERE id = ?;", [file_id])?;
            Ok(())
        })?;

        Ok(())
    }

    /// Fetches pending files sorted by priority indexing order:
    /// Screenshot (0) > Image (1) > PDF (2) > Doc (3) > Audio (4) > Video (5) > Code (6), then mtime DESC.
    pub fn pending_files(
        &self,
        limit: Option<usize>,
        only_ids: Option<&[i64]>,
    ) -> Result<Vec<FileRecord>, StorageError> {
        let mut query = format!(
            "SELECT id, path, kind, size, mtime, inode, device, state
             FROM files WHERE state = 'pending'"
        );

        if let Some(ids) = only_ids {
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let id_list = ids
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            query.push_str(&format!(" AND id IN ({})", id_list));
        }

        query.push_str(&format!(
            " ORDER BY {}, mtime DESC",
            FileKind::INDEXING_ORDER_SQL
        ));

        if let Some(lim) = limit {
            query.push_str(&format!(" LIMIT {}", lim));
        }

        let mut stmt = self.db.raw().prepare(&query)?;
        let mut rows = stmt.query([])?;

        let mut records = Vec::new();
        while let Some(row) = rows.next()? {
            records.push(Self::parse_file_row(row)?);
        }

        Ok(records)
    }

    /// Count of files currently awaiting indexing.
    pub fn pending_count(&self) -> Result<usize, StorageError> {
        let count: i64 = self.db.raw().query_row(
            "SELECT COUNT(*) FROM files WHERE state = 'pending';",
            [],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }

    // MARK: - Segment Operations

    /// Adds a content segment: inserts into `segments` (with fp16 vector blob) and `fts` (with normalized text).
    pub fn add_segment(
        &mut self,
        file_id: i64,
        kind: SegmentKind,
        modality: Modality,
        loc: Option<f64>,
        loc_end: Option<f64>,
        excerpt: Option<&str>,
        text: Option<&str>,
        vector: Option<&[f32]>,
    ) -> Result<i64, StorageError> {
        let seg_id: i64 = self.db.transaction(|tx| {
            let vec_blob = vector.map(Vectors::half);

            tx.execute(
                "INSERT INTO segments(file, kind, modality, loc, loc_end, excerpt, vec)
                 VALUES(?, ?, ?, ?, ?, ?, ?);",
                rusqlite::params![
                    file_id,
                    kind.as_str(),
                    if vector.is_some() {
                        modality.as_str()
                    } else {
                        "none"
                    },
                    loc,
                    loc_end,
                    excerpt,
                    vec_blob,
                ],
            )?;

            let id = tx.last_insert_rowid();

            // Insert into FTS virtual table if text content is present
            if let Some(raw_text) = text {
                if !raw_text.trim().is_empty() {
                    let norm = SearchText::normalized(raw_text);
                    let orig = if norm == raw_text {
                        None
                    } else {
                        Some(raw_text)
                    };

                    tx.execute(
                        "INSERT INTO fts(rowid, name, body, original) VALUES(?, '', ?, ?);",
                        rusqlite::params![id, norm, orig],
                    )?;
                }
            }

            Ok(id)
        })?;

        Ok(seg_id)
    }

    fn parse_file_row(row: &rusqlite::Row) -> Result<FileRecord, rusqlite::Error> {
        let kind_str: String = row.get(2)?;
        let kind = kind_str.parse::<FileKind>().unwrap_or(FileKind::Doc);

        Ok(FileRecord {
            id: row.get(0)?,
            path: row.get(1)?,
            kind,
            size: row.get(3)?,
            mtime: row.get(4)?,
            inode: row.get::<_, i64>(5)? as u64,
            device: row.get::<_, i64>(6)? as u64,
            state: row.get(7)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_insert_and_pending_order() {
        let mut store = IndexStore::open_in_memory().unwrap();

        // Insert code (priority 6)
        let _ = store
            .insert_file("/repo/main.rs", FileKind::Code, 1024, 100.0, 1, 1)
            .unwrap();
        // Insert screenshot (priority 0)
        let s_id = store
            .insert_file("/shots/shot.png", FileKind::Screenshot, 2048, 200.0, 2, 1)
            .unwrap();

        assert_eq!(store.pending_count().unwrap(), 2);

        // Pending files must return Screenshot first, even though Code was inserted first
        let pending = store.pending_files(None, None).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].id, s_id);
        assert_eq!(pending[0].kind, FileKind::Screenshot);
    }

    #[test]
    fn test_file_move_retains_segments() {
        let mut store = IndexStore::open_in_memory().unwrap();

        let fid = store
            .insert_file("/old/path/doc.pdf", FileKind::Pdf, 5000, 10.0, 999, 1)
            .unwrap();

        // Add a page segment with text and a mock 768-dim vector
        let mock_vec = vec![0.5f32; 768];
        let sid = store
            .add_segment(
                fid,
                SegmentKind::Page,
                Modality::Text,
                Some(1.0),
                Some(1.0),
                Some("Page 1 excerpt"),
                Some("Detailed quantum computing paper"),
                Some(&mock_vec),
            )
            .unwrap();

        // Move the file
        store.move_file(fid, "/new/path/doc.pdf").unwrap();

        // Verify path was updated
        let file = store
            .find_file_by_inode(1, 999)
            .unwrap()
            .expect("File must be found");
        assert_eq!(file.path, "/new/path/doc.pdf");

        // Verify vector and segment are still intact
        let vec_bytes: Vec<u8> = store
            .db()
            .raw()
            .query_row("SELECT vec FROM segments WHERE id = ?;", [sid], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(vec_bytes.len(), 1536);
    }

    #[test]
    fn test_delete_file_cascades() {
        let mut store = IndexStore::open_in_memory().unwrap();
        let fid = store
            .insert_file("/temp.txt", FileKind::Doc, 100, 1.0, 10, 1)
            .unwrap();
        let sid = store
            .add_segment(
                fid,
                SegmentKind::Chunk,
                Modality::Text,
                None,
                None,
                None,
                Some("Delete this test text"),
                None,
            )
            .unwrap();

        store.delete_file(fid).unwrap();

        // Verify file, segments, and fts rows are removed
        let f_count: i64 = store
            .db()
            .raw()
            .query_row("SELECT count(*) FROM files WHERE id = ?;", [fid], |r| {
                r.get(0)
            })
            .unwrap();
        let s_count: i64 = store
            .db()
            .raw()
            .query_row("SELECT count(*) FROM segments WHERE id = ?;", [sid], |r| {
                r.get(0)
            })
            .unwrap();
        let fts_count: i64 = store
            .db()
            .raw()
            .query_row("SELECT count(*) FROM fts WHERE rowid = ?;", [sid], |r| {
                r.get(0)
            })
            .unwrap();

        assert_eq!(f_count, 0);
        assert_eq!(s_count, 0);
        assert_eq!(fts_count, 0);
    }
}
