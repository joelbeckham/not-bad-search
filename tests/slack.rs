use anyhow::Result;
use good_search::{slack::{self,Provider,Settings},jira::Outcome};
use serde_json::{json,Value};
use std::collections::VecDeque;
fn envelope(body:&str,next:Option<&str>)->Value {json!({"content":[{"type":"text","text":serde_json::to_string(&json!({"messages":body,"pagination_info":next.map(|c|format!("More messages. Use cursor: `{c}`")).unwrap_or_else(||"There are no more messages available.\\n".into())})).unwrap()}]})}
fn message(ts:&str,body:&str)->String {format!("=== Message from Tester (U123) at today ===\nMessage TS: {ts}\n{body}")}
fn settings()->Settings {Settings{channel:"C12345678".into(),workspace:"https://example.slack.com".into(),since:"2026-01-01".into(),poll_seconds:300,..Default::default()}}
struct Fake(VecDeque<(&'static str,Value)>);
impl Provider for Fake {fn read(&mut self,tool:&str,args:Value)->Result<Value>{if tool=="slack_read_thread" {assert!(args.get("latest").is_none());}let (expected,v)=self.0.pop_front().expect("unexpected read"); assert_eq!(tool,expected); if v.is_null(){anyhow::bail!("temporary error")} Ok(v)}}
#[test]
fn page_failures_resume_and_edits_replace_content()->Result<()> {
 let mut db=good_search::open_db(std::path::Path::new(":memory:"))?;
 let one=message("1793980803.775709","oldneedle");
 let two=message("1793980804.775709","secondneedle");
 let mut fake=Fake(VecDeque::from([("slack_read_channel",envelope(&one,Some("next"))),("slack_read_channel",Value::Null)]));
 assert!(slack::sync(&mut db,"s",&settings(),&mut fake,||false).is_err());
 assert_eq!(db.query_row("SELECT next_token FROM remote_sync",[],|r|r.get::<_,String>(0))?,"next");
 assert_eq!(good_search::search(&db,"oldneedle",10)?.len(),1);
 fake.0.push_back(("slack_read_channel",envelope(&two,None)));
 assert_eq!(slack::sync(&mut db,"s",&settings(),&mut fake,||false)?,Outcome::Complete);
 fake.0.push_back(("slack_read_channel",envelope(&message("1793980803.775709","newneedle"),None)));
 slack::sync(&mut db,"s",&settings(),&mut fake,||false)?;
 assert!(good_search::search(&db,"oldneedle",10)?.is_empty());
 let hits=good_search::search(&db,"newneedle",10)?;
 assert!(hits[0].slack && hits[0].jira.is_none());
 assert_eq!(hits[0].open_label(),"Open in Slack");
 assert_eq!(hits[0].location(),"https://example.slack.com/archives/C12345678/p1793980803775709");
 assert_eq!(good_search::jira::open_target(&db,&hits[0].path)?,hits[0].location());
 Ok(())
}
#[test]
fn thread_and_image_pipeline_and_scope_filter()->Result<()> {
 use base64::Engine;
 let mut db=good_search::open_db(std::path::Path::new(":memory:"))?;
 let parent=message("1793980803.775709","parentneedle\nThread: 1 replies");
 let thread="=== THREAD PARENT MESSAGE ===\nFrom: Tester\nTime: today\nMessage TS: 1793980803.775709\nparentneedle\n=== THREAD REPLIES (1 total) ===\n--- Reply 1 of 1 ---\nFrom: Replier\nTime: today\nMessage TS: 1793980805.775709\nreplyneedle\nFiles: screenshot.png (ID: F12345678, image/png, 1 KB)";
 let mut fake=Fake(VecDeque::from([("slack_read_channel",envelope(&parent,None)),("slack_read_thread",envelope(thread,None))]));
 slack::sync(&mut db,"s",&settings(),&mut fake,||false)?;
 assert_eq!(good_search::search(&db,"replyneedle",10)?.len(),1);
 let dir=tempfile::tempdir()?; let image=dir.path().join("fixture.png"); image::RgbImage::new(8,8).save(&image)?;
 let bytes=std::fs::read(&image)?;
 fake.0.push_back(("slack_read_file",json!({"content":[{"type":"text","text":format!("[Image Response: {}, MIME: image/png]",base64::engine::general_purpose::STANDARD.encode(bytes))}]})));
 slack::sync_images(&mut db,"s",&mut fake,dir.path(),||false)?;
 let mut source=good_search::sources::Source::slack_source();source.id="s".into();source.paused=false;source.slack=Some(settings());
 let config=good_search::Config{sources:vec![source],..Default::default()};config.validate()?;
 good_search::images::discover(&mut db,&config)?;
 struct Extract;impl good_search::images::Extractor for Extract {fn extract(&self,_:&str,_:&str,_:&[u8])->Result<good_search::images::Extraction>{Ok(good_search::images::Extraction{extracted_text:"visualneedle".into(),description:"screenshot".into()})}}
 assert!(good_search::images::process_one(&mut db,&Extract,||config.clone())?);
 let hits=good_search::search(&db,"visualneedle",10)?;assert_eq!(hits.len(),1);assert!(hits[0].slack);assert!(hits[0].location().ends_with("p1793980805775709"));
 let filters=good_search::SearchFilters{roots:Some(vec!["jira:s".into()]),..Default::default()};assert!(good_search::search_filtered(&db,"visualneedle",10,filters)?.is_empty());
 let mut paused=config.clone();paused.sources[0].paused=true;assert!(paused.source_paused(std::path::Path::new("slack:s")));
 // Idempotent attachment refresh uses the cache and makes no additional file request.
 slack::sync_images(&mut db,"s",&mut fake,dir.path(),||false)?;
 Ok(())
}
#[test]
fn malformed_responses_and_cancellation_do_not_advance()->Result<()> {
 assert!(slack::parse_page(envelope("unknown format",None)).is_err());
 assert!(slack::parse_page(envelope(&message("invalid","text"),None)).is_err());
 assert!(slack::image_data(&json!({"content":[{"type":"text","text":"[Image Response: not base64, MIME: image/png]"}]})).is_err());
 let mut db=good_search::open_db(std::path::Path::new(":memory:"))?;
 let mut fake=Fake(VecDeque::new());assert_eq!(slack::sync(&mut db,"s",&settings(),&mut fake,||true)?,Outcome::Cancelled);
 assert_eq!(db.query_row("SELECT pages FROM remote_sync",[],|r|r.get::<_,i64>(0))?,0);
 let repeated=envelope(&message("1793980803.775709","text"),Some("same"));
 fake.0.extend([("slack_read_channel",repeated.clone()),("slack_read_channel",repeated)]);
 assert!(slack::sync(&mut db,"s",&settings(),&mut fake,||false).is_err());
 assert_eq!(db.query_row("SELECT pages FROM remote_sync",[],|r|r.get::<_,i64>(0))?,1);
 Ok(())
}

#[test]
fn history_and_update_cutoffs_cover_downtime_and_keep_old_content()->Result<()> {
 let mut s=settings();s.since="2016-01-01".into();s.update_lookback_days=30;
 let now=1800000000;
 let history=1451606400;
 assert_eq!(slack::scan_start(&s,None,now)?,history);
 assert_eq!(slack::scan_start(&s,Some(now-300),now)?,now-30*86400);
 assert_eq!(slack::scan_start(&s,Some(now-90*86400),now)?,now-90*86400-1);
 s.update_lookback_days=36500;assert_eq!(slack::scan_start(&s,Some(now-300),now)?,history);
 let legacy:Settings=serde_json::from_value(json!({"channel":"C12345678","workspace":"https://example.slack.com","since":"2016-01-01","poll_seconds":300}))?;
 assert_eq!(legacy.update_lookback_days,30);legacy.validate()?;
 s.update_lookback_days=0;assert!(s.validate().is_err());
 Ok(())
}

#[test]
fn incremental_bounds_resume_without_skipping_and_full_reload_resets()->Result<()> {
 struct Recorder {responses:VecDeque<Value>, calls:Vec<Value>}
 impl Provider for Recorder {fn read(&mut self,_:&str,args:Value)->Result<Value>{self.calls.push(args);let response=self.responses.pop_front().unwrap();if response.is_null(){anyhow::bail!("retry")}Ok(response)}}
 let mut db=good_search::open_db(std::path::Path::new(":memory:"))?;
 let mut s=settings();s.since="2016-01-01".into();
 let old=message("1451692800.000001","historicneedle");
 let mut provider=Recorder{responses:VecDeque::from([envelope(&old,Some("page2")),Value::Null]),calls:vec![]};
 assert!(slack::sync(&mut db,"s",&s,&mut provider,||false).is_err());
 assert_eq!(provider.calls[0]["oldest"],"1451606400");
 assert_eq!(db.query_row("SELECT checkpoint FROM remote_sync",[],|r|r.get::<_,Option<String>>(0))?,None);
 // Changing the update lookback does not change the bounds of the page in flight.
 s.update_lookback_days=7;
 provider.responses.push_back(envelope("No messages",None));
 slack::sync(&mut db,"s",&s,&mut provider,||false)?;
 assert_eq!(provider.calls[2]["cursor"],"page2");
 assert_eq!(provider.calls[0]["oldest"],provider.calls[2]["oldest"]);
 assert_eq!(provider.calls[0]["latest"],provider.calls[2]["latest"]);
 let checkpoint=db.query_row("SELECT checkpoint FROM remote_sync",[],|r|r.get::<_,String>(0))?;
 assert_eq!(provider.calls[0]["latest"],checkpoint);
 // The next successful scan is incremental, even when the polling interval changes.
 s.poll_seconds=600;
 provider.responses.push_back(envelope("No messages",None));
 slack::sync(&mut db,"s",&s,&mut provider,||false)?;
 let call=&provider.calls[3];let upper=call["latest"].as_str().unwrap().parse::<i64>()?;
 assert_eq!(call["oldest"].as_str().unwrap().parse::<i64>()?,slack::scan_start(&s,Some(checkpoint.parse()?),upper)?);
 assert_eq!(good_search::search(&db,"historicneedle",10)?.len(),1);
 // Explicit reload rechecks the full date range, preserving existing indexed results.
 s.reload_generation+=1;
 provider.responses.push_back(envelope("No messages",None));
 slack::sync(&mut db,"s",&s,&mut provider,||false)?;
 assert_eq!(provider.calls[4]["oldest"],"1451606400");
 assert_eq!(good_search::search(&db,"historicneedle",10)?.len(),1);
 Ok(())
}
