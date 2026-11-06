-- Remote items use the existing search/document pipeline through a backing files row.
-- Its path is a stable internal URI, never a path to read from the filesystem.
CREATE TABLE remote_items(
 file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
 source_id TEXT NOT NULL, connection_id TEXT NOT NULL, site TEXT NOT NULL,
 remote_id TEXT NOT NULL, issue_key TEXT NOT NULL, title TEXT NOT NULL,
 original_url TEXT NOT NULL, remote_updated TEXT NOT NULL, synced_at TEXT NOT NULL,
 content_hash TEXT NOT NULL, raw_adf TEXT NOT NULL, searchable_text TEXT NOT NULL,
 warnings TEXT NOT NULL, last_seen_run TEXT NOT NULL, scope_hash TEXT NOT NULL,
 UNIQUE(source_id,site,remote_id)
);
CREATE INDEX remote_source ON remote_items(source_id);
CREATE TABLE remote_failures(
 source_id TEXT NOT NULL, site TEXT NOT NULL, remote_id TEXT NOT NULL,
 issue_key TEXT NOT NULL,error TEXT NOT NULL,attempted_at TEXT NOT NULL,
 PRIMARY KEY(source_id,site,remote_id)
);
CREATE TABLE remote_sync(
 source_id TEXT PRIMARY KEY, scope_hash TEXT NOT NULL,
 checkpoint TEXT, run_id TEXT, run_jql TEXT, next_token TEXT, candidate TEXT,
 last_success TEXT, last_attempt TEXT, error TEXT NOT NULL DEFAULT '',
 failures INTEGER NOT NULL DEFAULT 0, retry_at INTEGER NOT NULL DEFAULT 0,
 pages INTEGER NOT NULL DEFAULT 0, items INTEGER NOT NULL DEFAULT 0
);
