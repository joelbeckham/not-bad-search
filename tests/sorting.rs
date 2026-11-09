use anyhow::Result;
use good_search::{Hit,SearchFilters,sorting::{self,SortMode}};
fn hit(path:&str,timestamp:Option<i64>)->Hit { Hit{path:path.into(),name:path.into(),timestamp,original_url:None,jira:None,gmail:None,notion:false,slack:false,slack_replies:None,snippet:None} }
#[test]
fn dates_and_modest_boost_are_deterministic() {
 let now=1800000000;
 let old=now-86400*3650;
 let original=vec![hit("old",Some(old)),hit("new",Some(now)),hit("unknown",None)];
 let mut hits=original.clone();sorting::sort(&mut hits,"",SortMode::NewestFirst,now);assert_eq!(hits[0].path,"new");assert_eq!(hits[2].path,"unknown");
 sorting::sort(&mut hits,"",SortMode::OldestFirst,now);assert_eq!(hits[0].path,"old");assert_eq!(hits[2].path,"unknown");
 hits=original.clone();sorting::sort(&mut hits,"",SortMode::PreferNewer,now);assert_eq!(hits[0].path,"new");
 hits=original;sorting::sort(&mut hits,"old",SortMode::PreferNewer,now);assert_eq!(hits[0].path,"old");
 let mut many:Vec<_>=(0..50).map(|i|hit(&format!("old{i}"),Some(old))).collect();many.push(hit("weak_new",Some(now)));
 sorting::sort(&mut many,"",SortMode::PreferNewer,now);assert_ne!(many[0].path,"weak_new");
}
#[test]
fn date_sort_applies_before_limits_and_includes_content_matches()->Result<()> {
 let db=good_search::open_db(std::path::Path::new(":memory:"))?;
 for i in 1..=210 {
  db.execute("INSERT INTO files(id,root,path,name,folded) VALUES(?1,'root',?2,?2,?2)",rusqlite::params![i,format!("needle-{i:03}")])?;
  db.execute("INSERT INTO file_times VALUES(?1,?1)",[i])?;
 }
 db.execute_batch("INSERT INTO files VALUES(211,'root','body-only','body-only','body-only'); INSERT INTO file_times VALUES(211,999); INSERT INTO documents(file_id,kind,source_hash,extractor_version,chunker_version,status,detail) VALUES(211,'text','hash',1,1,'ready',''); INSERT INTO chunks(document_id,ordinal,heading,start_line,body,content_hash) VALUES(last_insert_rowid(),0,'',1,'needle body','hash');")?;
 let filters=SearchFilters{sort:SortMode::NewestFirst,..Default::default()};
 let hits=good_search::search_filtered(&db,"needle",3,filters)?;
 assert_eq!(hits.iter().map(|h|h.path.as_str()).collect::<Vec<_>>(),["body-only","needle-210","needle-209"]);
 let filters=SearchFilters{sort:SortMode::OldestFirst,..Default::default()};
 assert_eq!(good_search::search_filtered(&db,"needle",3,filters)?[0].path,"needle-001");
 Ok(())
}
#[test]
fn folder_modified_dates_are_indexed()->Result<()> {
 let dir=tempfile::tempdir()?;let path=dir.path().join("screenshot.png");std::fs::write(&path,"fixture")?;
 let when=std::time::UNIX_EPOCH+std::time::Duration::from_secs(1600000000);
 std::fs::File::options().write(true).open(&path)?.set_times(std::fs::FileTimes::new().set_modified(when))?;
 let mut db=good_search::open_db(std::path::Path::new(":memory:"))?;
 let config=good_search::Config{folders:vec![dir.path().into()],..Default::default()};
 good_search::reconcile(&mut db,&config)?;
 assert_eq!(good_search::search(&db,"screenshot",10)?[0].timestamp,Some(1600000000));
 Ok(())
}
