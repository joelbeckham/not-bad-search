use anyhow::Result;
use not_bad_search::{open_db, open_reader, open_writer};
use rusqlite::TransactionBehavior;
use std::{sync::mpsc, time::Duration};

#[test]
fn deferred_read_then_write_reproduces_snapshot_lock() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("index.sqlite");
    let mut first = open_db(&path)?;
    first.execute_batch("CREATE TABLE counter(value INTEGER); INSERT INTO counter VALUES(0);")?;
    let second = open_writer(&path)?;
    let tx = first.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let _: i64 = tx.query_row("SELECT value FROM counter", [], |r| r.get(0))?;
    second.execute("UPDATE counter SET value=value+1", [])?;
    let error = tx
        .execute("UPDATE counter SET value=value+1", [])
        .unwrap_err();
    assert_eq!(
        error.sqlite_error().unwrap().extended_code,
        rusqlite::ffi::SQLITE_BUSY_SNAPSHOT
    );
    Ok(())
}

#[test]
fn writers_wait_before_reading_while_search_readers_remain_available() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("index.sqlite");
    let mut first = open_db(&path)?;
    first.execute_batch("CREATE TABLE counter(value INTEGER); INSERT INTO counter VALUES(0);")?;
    let reader = open_reader(&path)?;
    let mut second = open_writer(&path)?;
    let tx = first.transaction()?;
    let _: i64 = tx.query_row("SELECT value FROM counter", [], |r| r.get(0))?;
    let (started_tx, started_rx) = mpsc::channel();
    let (acquired_tx, acquired_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || -> Result<()> {
        started_tx.send(())?;
        let tx = second.transaction()?;
        acquired_tx.send(())?;
        let value: i64 = tx.query_row("SELECT value FROM counter", [], |r| r.get(0))?;
        assert_eq!(value, 1);
        tx.execute("UPDATE counter SET value=value+1", [])?;
        tx.commit()?;
        Ok(())
    });
    started_rx.recv_timeout(Duration::from_secs(5))?;
    assert!(matches!(
        acquired_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    assert_eq!(
        reader.query_row("SELECT value FROM counter", [], |r| r.get::<_, i64>(0))?,
        0
    );
    tx.execute("UPDATE counter SET value=value+1", [])?;
    tx.commit()?;
    acquired_rx.recv_timeout(Duration::from_secs(5))?;
    worker.join().unwrap()?;
    assert_eq!(
        reader.query_row("SELECT value FROM counter", [], |r| r.get::<_, i64>(0))?,
        2
    );
    Ok(())
}
