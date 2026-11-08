ALTER TABLE remote_items RENAME COLUMN raw_adf TO raw_content;
ALTER TABLE remote_items ADD COLUMN content_format TEXT NOT NULL DEFAULT 'adf';
-- Re-read full scopes without deleting the last usable local snapshot.
UPDATE remote_sync SET checkpoint=NULL,run_id=NULL,run_jql=NULL,next_token=NULL,candidate=NULL,last_success=NULL,scope_hash='',retry_at=0,failures=0;
