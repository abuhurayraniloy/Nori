use rusqlite::{Connection, Result};

pub fn init_db() -> Result<Connection> {
    // Connects to (or create) a local sql file named nori.db
    let conn = Connection::open("nori.db")?;

    // Create the files table if it does not exist yet
    conn.execute(
        "CREATE TABLE IF NOT EXISTS files (
            id TEXT PRIMARY KEY,
            current_path TEXT NOT NULL,
            original_path TEXT NOT NULL,
            filename TEXT NOT NULL,
            created_at DATETIME NOT NULL
        )",
        [],
    )?;

    // Create the transcations table for one-click undo feature
    conn.execute(
        "CREATE TABLE IF NOT EXISTS transactions (
            transaction_id TEXT PRIMARY KEY,
            source_path TEXT NOT NULL,
            destination_path TEXT NOT NULL,
            timestamp DATETIME NOT NULL,
            is_reverted INTEGER DEFAULT 0
        )",
        [],
    )?;

    Ok(conn)
}
