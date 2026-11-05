use anyhow::{bail, Result};
use rusqlite::{Connection, OpenFlags};
use std::{path::Path, time::Duration};

pub fn open_writer(path: &Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
    Ok(db)
}

pub fn open_reader(path: &Path) -> Result<Connection> {
    let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.busy_timeout(Duration::from_millis(100))?;
    db.execute_batch("PRAGMA foreign_keys=ON; PRAGMA query_only=ON;")?;
    Ok(db)
}

/// Run migrations once at startup, before starting workers.
pub fn open_db(path: &Path) -> Result<Connection> {
    let mut db = open_writer(path)?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version > 1 {
        bail!("Index schema {version} is newer than this application supports");
    }
    db.execute_batch("PRAGMA journal_mode=WAL;")?;
    if version == 0 {
        let tx = db.transaction()?;
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    Ok(db)
}
