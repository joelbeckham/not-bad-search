use anyhow::{bail, Result};
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::{path::Path, time::Duration};

pub fn open_writer(path: &Path) -> Result<Connection> {
    let mut db = Connection::open(path)?;
    db.busy_timeout(Duration::from_secs(5))?;
    // Reserve the writer before reading a transaction snapshot. A deferred
    // read-to-write upgrade can fail with SQLITE_BUSY_SNAPSHOT immediately,
    // bypassing busy_timeout when another indexing worker commits in between.
    db.set_transaction_behavior(TransactionBehavior::Immediate);
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
    if version > 3 {
        bail!("Index schema {version} is newer than this application supports");
    }
    db.execute_batch("PRAGMA journal_mode=WAL;")?;
    if version == 0 {
        let tx = db.transaction()?;
        tx.execute_batch(include_str!("schema.sql"))?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }
    if version < 2 {
        // Disable outside the transaction so rebuilding the parent preserves chunks.
        db.pragma_update(None, "foreign_keys", false)?;
        let result = (|| -> Result<()> {
            let tx = db.transaction()?;
            tx.execute_batch(include_str!("schema-v2.sql"))?;
            let violations: i64 =
                tx.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get(0)
                })?;
            if violations != 0 {
                bail!("Foreign key validation failed during migration");
            }
            tx.pragma_update(None, "user_version", 2)?;
            tx.commit()?;
            Ok(())
        })();
        db.pragma_update(None, "foreign_keys", true)?;
        result?;
    }
    if version < 3 {
        let tx = db.transaction()?;
        tx.execute_batch(include_str!("schema-v3.sql"))?;
        tx.pragma_update(None, "user_version", 3)?;
        tx.commit()?;
    }
    Ok(db)
}
