use anyhow::Result;
use good_search::{inspection::inspect, open_db, open_reader};

#[test]
fn complete_read_only_snapshot_and_missing_file() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("index.sqlite");
    let db = open_db(&path)?;
    db.execute_batch(
        "INSERT INTO files VALUES(42,'root','test.png','test.png','test.png');
        INSERT INTO documents VALUES(10,42,'image_text','hash',1,1,'ready','');
        INSERT INTO documents VALUES(11,42,'image_description','hash',1,1,'ready','');
        INSERT INTO chunks VALUES(20,10,0,'heading',1,'first line
second line','chunkhash');
        INSERT INTO chunks VALUES(21,11,0,'',1,'blue diagram','descriptionhash');
        INSERT INTO image_jobs VALUES(42,'hash','model',1,'done',2,0,'');
        INSERT INTO image_cache VALUES('hash','model',1,'first line
second line','blue diagram');",
    )?;
    let mut reader = open_reader(&path)?;
    let before: i64 = db.query_row("SELECT total_changes()", [], |r| r.get(0))?;
    let value = inspect(&mut reader, "test.png")?;
    assert_eq!(value["file"]["id"], 42);
    assert_eq!(value["documents"].as_array().unwrap().len(), 2);
    let text = value["documents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["kind"] == "image_text")
        .unwrap();
    assert_eq!(text["chunks"][0]["body"], "first line\nsecond line");
    assert_eq!(text["chunks"][0]["content_hash"], "chunkhash");
    assert_eq!(value["image_jobs"][0]["attempts"], 2);
    assert_eq!(value["image_cache"][0]["description"], "blue diagram");
    assert!(inspect(&mut reader, "missing.png").is_err());
    assert!(inspect(&mut reader, "' OR 1=1 --").is_err());
    assert_eq!(
        before,
        db.query_row("SELECT total_changes()", [], |r| r.get::<_, i64>(0))?
    );
    db.execute("DELETE FROM files", [])?;
    assert!(inspect(&mut reader, "test.png").is_err());
    Ok(())
}
