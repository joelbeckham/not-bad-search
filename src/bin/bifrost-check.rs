//! Explicit console connection probe. Never prints credentials or endpoint.
use anyhow::{Context, Result};
use good_search::bifrost::{Connection, Mcp};
use serde_json::json;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "status") {
        let path = std::path::Path::new(args.get(2).context("Data directory required")?);
        let db = good_search::open_reader(&path.join("index.sqlite"))?;
        let mut statement=db.prepare("SELECT r.issue_key,r.remote_updated,r.synced_at,length(r.raw_content),(SELECT count(*) FROM chunks c JOIN documents d ON d.id=c.document_id WHERE d.file_id=r.file_id),(SELECT count(*) FROM embeddings e JOIN chunks c ON c.id=e.chunk_id JOIN documents d ON d.id=c.document_id WHERE d.file_id=r.file_id) FROM remote_items r ORDER BY r.issue_key")?;
        for row in statement.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })? {
            println!("{:?}", row?);
        }
        let config: good_search::Config =
            toml::from_str(&std::fs::read_to_string(path.join("config.toml"))?)?;
        for status in good_search::sources::snapshot(&db, &config)? {
            if let Some(message) = status.remote_status {
                println!("{message}");
            }
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "fixture") {
        return fixture(std::path::Path::new(
            args.get(2).context("New fixture directory required")?,
        ));
    }
    if args.get(1).is_some_and(|s| s == "attach") {
        let from = std::path::Path::new(args.get(2).context("Source test directory required")?);
        let to = std::path::Path::new(args.get(3).context("Destination data directory required")?);
        let fixture: good_search::Config =
            toml::from_str(&std::fs::read_to_string(from.join("config.toml"))?)?;
        let mut config: good_search::Config =
            toml::from_str(&std::fs::read_to_string(to.join("config.toml"))?)?;
        config.migrate_sources();
        let mut source = fixture
            .sources
            .into_iter()
            .find(|s| !s.is_folder())
            .context("No Jira source")?;
        source.paused = false;
        anyhow::ensure!(
            !config.sources.iter().any(|s| s.id == source.id),
            "Source already attached"
        );
        config.sources.push(source);
        config.sync_sources();
        config.validate()?;
        std::fs::write(to.join("config.toml"), toml::to_string_pretty(&config)?)?;
        println!("Added the pinned Jira source; existing sources and settings preserved");
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "verify") {
        let path = std::path::Path::new(args.get(2).context("Data directory required")?);
        let config: good_search::Config =
            toml::from_str(&std::fs::read_to_string(path.join("config.toml"))?)?;
        let mut db = good_search::open_db(&path.join("index.sqlite"))?;
        let ids = |db: &rusqlite::Connection| -> Result<Vec<i64>> {
            Ok(db
                .prepare("SELECT id FROM chunks ORDER BY id")?
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?)
        };
        let before = ids(&db)?;
        let mut m = Mcp::connect(Connection::load()?)?;
        for source in config.sources.iter().filter(|s| !s.is_folder()) {
            let result = good_search::jira::sync(
                &mut db,
                &source.id,
                source.jira.as_ref().context("No settings")?,
                &mut m,
                || false,
            )?;
            println!("Incremental sync: {result:?}");
        }
        println!("Chunk IDs preserved: {}", before == ids(&db)?);
        let mut times = vec![];
        for _ in 0..100 {
            let start = std::time::Instant::now();
            let hits = good_search::search(&db, "CORE", 100)?;
            anyhow::ensure!(
                hits.len() == 2 && hits.iter().all(|h| h.original_url.is_some()),
                "Expected two Jira results with original URLs"
            );
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(f64::total_cmp);
        println!("Local search ms p50={:.3} p95={:.3}", times[50], times[95]);
        if let Some(model) = args.get(3) {
            let local = good_search::semantic::LocalModel::load(std::path::Path::new(model))?;
            while good_search::semantic::index_batch(&mut db, &local, 64)? > 0 {}
            let mut index = good_search::semantic::VectorIndex::default();
            index.refresh(&db)?;
            let query: String =
                db.query_row("SELECT body FROM chunks ORDER BY id LIMIT 1", [], |r| {
                    r.get(0)
                })?;
            let vector = good_search::semantic::embed_query(
                &local,
                &query.chars().take(180).collect::<String>(),
            )?;
            let hits = index.search(&db, vector, 10)?;
            anyhow::ensure!(
                !hits.is_empty() && hits.iter().all(|h| h.original_url.is_some()),
                "Semantic search did not resolve Jira result locations"
            );
            println!(
                "Local embedding pipeline: {} chunks; {} semantic matches",
                ids(&db)?.len(),
                hits.len()
            );
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "import") {
        let path = args.get(2).context("Private input file required")?;
        let connection: Connection = serde_json::from_slice(&std::fs::read(path)?)?;
        connection.save()?;
        println!("Bifrost connection saved to Windows Credential Manager");
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "resolve") {
        let saved = Connection::load()?;
        let base = reqwest::Url::parse(&saved.endpoint)?;
        println!("Supplied endpoint is site root: {}", base.path() == "/");
        {
            let path = "/mcp";
            let candidate = Connection {
                endpoint: base.join(path)?.to_string(),
                api_key: saved.api_key.clone(),
            };
            match Mcp::connect(candidate) {
                Ok(m) => {
                    println!(
                        "Discovered read tools at {path}: {}",
                        serde_json::to_string(&m.catalog())?
                    );
                    Connection {
                        endpoint: base.join(path)?.to_string(),
                        api_key: saved.api_key.clone(),
                    }
                    .save()?;
                    println!("Saved resolved MCP endpoint securely");
                    return Ok(());
                }
                Err(e) => println!("{path}: {e}"),
            }
        }
        anyhow::bail!("Could not resolve an MCP endpoint on the supplied origin");
    }
    let mut m = Mcp::connect_discovery(Connection::load()?)?;
    if args.get(1).is_some_and(|s|s=="format-probe") {
        let page=m.call("jira_searchJiraIssuesUsingJql",json!({"cloudId":"bombbomb.atlassian.net","jql":"project = CORE ORDER BY updated ASC","maxResults":50,"fields":["id","key","summary","description","updated"],"searchResultMode":"issues","view":"full","responseContentFormat":"markdown"}))?;
        println!("Page format {}",page["appliedContentFormat"]);
        for key in ["CORE-6","CORE-7"] {
            use good_search::jira::Provider;
            let value=m.issue(&Default::default(),key)?;
            println!("Detail {key}: format={}, description_null={}",value["appliedContentFormat"],value["fields"]["description"].is_null());
        }
        for issue in page["issues"].as_array().context("Missing issues")? {
            println!("{} issue-format={} field-format={} field-keys={:?}",issue["key"],issue["appliedContentFormat"],issue["fields"]["appliedContentFormat"],issue["fields"].as_object().map(|o|o.keys().collect::<Vec<_>>()));
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "attachment-test") {
        let path=std::path::Path::new(args.get(2).context("Fixture directory required")?);
        std::fs::create_dir_all(path)?;
        let mut db=good_search::open_db(&path.join("index.sqlite"))?;
        let mut source=good_search::sources::Source::jira_source();source.id="attachment-test".into();source.paused=false;
        let settings=source.jira.as_mut().unwrap();settings.jql="key in (CORE-11116, CORE-11112)".into();settings.page_size=1;
        println!("Sync: {:?}",good_search::jira::sync(&mut db,&source.id,settings,&mut m,||false)?);
        good_search::attachments::sync_jira(&mut db,&source.id,settings,&mut m,&path.join("attachments"),||false)?;
        let config=good_search::Config{sources:vec![source],semantic_enabled:false,..Default::default()};
        std::fs::write(path.join("config.toml"),toml::to_string_pretty(&config)?)?;
        good_search::images::discover(&mut db,&config)?;
        if args.get(3).is_some_and(|s|s=="analyze") {
            let extractor=good_search::images::OpenAi::new()?;
            for _ in 0..2 {good_search::images::process_one(&mut db,&extractor,||config.clone())?;}
        }
        println!("Images: {}",good_search::images::summary(&db)?);
        let mut formats=db.prepare("SELECT content_format,count(*) FROM remote_items GROUP BY content_format")?;
        for row in formats.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))? {println!("Stored format: {:?}",row?);}
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "discovery") {
        for name in m.discovered_tools() {
            if name.starts_with("jira-") || name.starts_with("jira_v2") {
                println!("{name}");
            }
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "slack-smoke") {
        let dir=std::path::Path::new(args.get(2).context("New fixture directory required")?);
        anyhow::ensure!(!dir.exists(),"Fixture directory already exists");
        std::fs::create_dir_all(dir)?;
        let settings=good_search::slack::Settings {channel:args.get(3).context("Channel required")?.clone(),workspace:args.get(4).context("Workspace URL required")?.clone(),since:args.get(5).context("Start date required")?.clone(),poll_seconds:300,..Default::default()};
        let mut db=good_search::open_db(&dir.join("index.sqlite"))?;
        let outcome=good_search::slack::sync(&mut db,"slack-smoke",&settings,&mut m,||false)?;
        good_search::slack::sync_images(&mut db,"slack-smoke",&mut m,&dir.join("attachments"),||false)?;
        let counts:(i64,i64)=db.query_row("SELECT (SELECT count(*) FROM remote_items),(SELECT count(*) FROM attachments)",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
        println!("Slack sync {outcome:?}; {} messages, {} cached images",counts.0,counts.1);
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "slack-probe") { let tool=args.get(2).context("Read tool required")?; let arguments=serde_json::from_str(args.get(3).context("JSON arguments required")?)?; println!("{}",serde_json::to_string_pretty(&m.slack_read(tool,arguments)?)?); return Ok(()); }
    if args.get(1).is_some_and(|s| s == "slack-catalog") { println!("{}", serde_json::to_string_pretty(&m.v2_catalog().as_object().unwrap().iter().filter(|(k,_)| k.to_lowercase().contains("slack")).map(|(k,v)|(k.clone(),v.clone())).collect::<serde_json::Map<_,_>>())?); return Ok(()); }
    if args.get(1).is_some_and(|s| s == "v2-catalog") {
        println!(
            "{}",
            serde_json::to_string_pretty(&m.v2_catalog()["jira_v2-downloadJiraIssueAttachment"])?
        );
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "v2-download-probe") {
        let result = m.probe_v2_download(
            json!({"cloudId":"bombbomb.atlassian.net","attachmentId":"76578"}),
        )?;
        println!(
            "Response fields: {:?}",
            result.as_object().map(|o| o.keys().collect::<Vec<_>>())
        );
        fn urls(value: &serde_json::Value, found: &mut Vec<String>) {
            match value {
                serde_json::Value::String(s)
                    if s.starts_with("https://") && !s.contains(char::is_whitespace) =>
                {
                    found.push(s.clone())
                }
                serde_json::Value::Object(o) => {
                    for v in o.values() {
                        urls(v, found);
                    }
                }
                serde_json::Value::Array(a) => {
                    for v in a {
                        urls(v, found);
                    }
                }
                _ => {}
            }
        }
        let mut found = Vec::new();
        urls(&result, &mut found);
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        for url in found {
            let parsed = reqwest::Url::parse(&url)?;
            let host = parsed.host_str().unwrap_or("");
            if !(host.ends_with(".atlassian.com")
                || host.ends_with(".atl-paas.net")
                || host.ends_with(".atlassian.net"))
            {
                println!("Skipped unrecognized download host {host}");
                continue;
            }
            let response = client
                .get(url)
                .send()
                .map_err(|_| anyhow::anyhow!("Download request failed"))?;
            println!(
                "Download host={host} HTTP={} type={:?}",
                response.status().as_u16(),
                response.headers().get("content-type")
            );
            if response.status().is_success() {
                use std::io::Read;
                let mut bytes = Vec::new();
                response.take(1024 * 1024).read_to_end(&mut bytes)?;
                println!(
                    "Received {} bytes; PNG signature={}",
                    bytes.len(),
                    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                );
            }
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "image-probe") {
        let result = m.call("jira_searchJiraIssuesUsingJql", json!({
            "cloudId":"bombbomb.atlassian.net", "jql":"project = CORE AND attachments IS NOT EMPTY ORDER BY updated DESC",
            "maxResults":2,"fields":["attachment"],"searchResultMode":"issues"
        }))?;
        for issue in result["issues"].as_array().context("Missing issues")? {
            println!("Issue {}", issue["key"]);
            for attachment in issue["fields"]["attachment"]
                .as_array()
                .into_iter()
                .flatten()
            {
                if !attachment["mimeType"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("image/")
                {
                    continue;
                }
                let url = attachment["content"]
                    .as_str()
                    .context("Missing download URL")?;
                let parsed = reqwest::Url::parse(url)?;
                println!(
                    "Image id={} type={} size={} download_host={:?}",
                    attachment["id"],
                    attachment["mimeType"],
                    attachment["size"],
                    parsed.host_str()
                );
                // No gateway credentials are ever sent to attachment hosts.
                let response = reqwest::blocking::Client::builder()
                    .redirect(reqwest::redirect::Policy::none())
                    .timeout(std::time::Duration::from_secs(15))
                    .build()?
                    .get(url)
                    .send()
                    .map_err(|_| anyhow::anyhow!("Attachment request failed"))?;
                println!(
                    "Unauthenticated download HTTP {}",
                    response.status().as_u16()
                );
            }
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "catalog") {
        println!("{}", serde_json::to_string_pretty(&m.catalog())?);
        return Ok(());
    }
    let user = m.call("jira_atlassianUserInfo", json!({}))?;
    println!(
        "Identity: {}",
        user.get("displayName")
            .or(user.get("name"))
            .unwrap_or(&serde_json::Value::Null)
    );
    let mut query = json!({"cloudId":"bombbomb.atlassian.net","jql":"project = CORE ORDER BY updated DESC","maxResults":2,"fields":["id","key","summary","description","updated"],"searchResultMode":"issues","responseContentFormat":"adf"});
    let page = match m.call("jira_searchJiraIssuesUsingJql", query.clone()) {
        Ok(page) => page,
        Err(error) => {
            println!("Search with ADF option failed: {error}. Retrying without the option.");
            query
                .as_object_mut()
                .unwrap()
                .remove("responseContentFormat");
            m.call("jira_searchJiraIssuesUsingJql", query)?
        }
    };
    for issue in page["issues"].as_array().context("No issues array")? {
        println!(
            "Issue {}: description_kind={} bytes={}",
            issue["key"],
            if issue["fields"]["description"].is_object() {
                "adf"
            } else {
                "string_or_null"
            },
            serde_json::to_vec(&issue["fields"]["description"])?.len()
        );
    }
    println!(
        "isLast={} nextPageToken_present={}",
        page["isLast"],
        page.get("nextPageToken").is_some()
    );
    if args.get(1).is_some_and(|s| s == "seed") {
        let path = std::path::Path::new(args.get(2).context("New data directory required")?);
        anyhow::ensure!(
            !path.exists(),
            "Use a new data directory for the live two-issue test"
        );
        let keys = page["issues"]
            .as_array()
            .context("No issues")?
            .iter()
            .take(2)
            .map(|i| i["key"].as_str().context("Missing key"))
            .collect::<Result<Vec<_>>>()?;
        anyhow::ensure!(
            keys.len() == 2
                && keys.iter().all(|k| k.starts_with("CORE-")
                    && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')),
            "Expected two CORE issue keys"
        );
        let mut source = good_search::sources::Source::jira_source();
        source.name = "CORE · Jira test".into();
        let settings = source.jira.as_mut().unwrap();
        settings.jql = format!("key in ({})", keys.join(", "));
        settings.page_size = 1;
        println!("Pinned JQL: {}", settings.jql);
        std::fs::create_dir_all(path)?;
        let config = good_search::Config {
            sources: vec![source.clone()],
            semantic_enabled: false,
            ..Default::default()
        };
        std::fs::write(path.join("config.toml"), toml::to_string_pretty(&config)?)?;
        let mut db = good_search::open_db(&path.join("index.sqlite"))?;
        let result = good_search::jira::sync(
            &mut db,
            &source.id,
            source.jira.as_ref().unwrap(),
            &mut m,
            || false,
        )?;
        let count: i64 = db.query_row("SELECT count(*) FROM remote_items", [], |r| r.get(0))?;
        println!("Live import: {result:?}, {count} locally stored issues; source saved paused");
    }
    Ok(())
}

fn fixture(path: &std::path::Path) -> Result<()> {
    anyhow::ensure!(!path.exists(), "Use a new fixture directory");
    std::fs::create_dir_all(path)?;
    let mut source = good_search::sources::Source::jira_source();
    source.id = "jira-fixture".into();
    source.name = "CORE test issues".into();
    source.jira.as_mut().unwrap().jql = "key in (CORE-1, CORE-2)".into();
    let mut db = good_search::open_db(&path.join("index.sqlite"))?;
    struct Fixture;
    impl good_search::jira::Provider for Fixture {
        fn page(
            &mut self,
            _: &good_search::jira::Settings,
            _: &str,
            _: Option<&str>,
        ) -> Result<serde_json::Value> {
            Ok(
                json!({"isLast":true,"issues":([1,2].into_iter().map(|id|json!({"id":id.to_string(),"key":format!("CORE-{id}"),"fields":{"summary":format!("Jira synthetic preview {id}"),"updated":"2026-11-06T12:00:00Z","description":{"version":1,"type":"doc","content":[{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Local Jira content"}]},{"type":"paragraph","content":[{"type":"text","text":"This synthetic issue tests local search, preview, metadata inspection and Jira source settings."}]},{"type":"paragraph","content":[{"type":"text","text":"Reference","marks":[{"type":"link","attrs":{"href":"https://example.com/reference"}}]}]}]}}})).collect::<Vec<_>>())}),
            )
        }
        fn issue(&mut self, _: &good_search::jira::Settings, _: &str) -> Result<serde_json::Value> {
            anyhow::bail!("Fixture descriptions are already ADF")
        }
    }
    good_search::jira::sync(
        &mut db,
        &source.id,
        source.jira.as_ref().unwrap(),
        &mut Fixture,
        || false,
    )?;
    let config = good_search::Config {
        sources: vec![source],
        semantic_enabled: false,
        ..Default::default()
    };
    std::fs::write(path.join("config.toml"), toml::to_string_pretty(&config)?)?;
    println!("Created isolated, paused synthetic Jira fixture");
    Ok(())
}
