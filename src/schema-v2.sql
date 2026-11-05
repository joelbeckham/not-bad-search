CREATE TABLE documents_new(
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
 kind TEXT NOT NULL CHECK(kind IN ('text','image_text','image_description')),
 source_hash TEXT NOT NULL,
 extractor_version INTEGER NOT NULL,
 chunker_version INTEGER NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('ready','skipped','error')),
 detail TEXT NOT NULL,
 UNIQUE(file_id,kind)
);
INSERT INTO documents_new SELECT id,file_id,'text',source_hash,extractor_version,chunker_version,status,detail FROM documents;
DROP TABLE documents;
ALTER TABLE documents_new RENAME TO documents;
CREATE TABLE image_jobs(
 file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
 source_hash TEXT NOT NULL, model TEXT NOT NULL, version INTEGER NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('pending','running','done','failed','skipped')),
 attempts INTEGER NOT NULL DEFAULT 0,
 retry_at INTEGER NOT NULL DEFAULT 0,
 detail TEXT NOT NULL DEFAULT ''
);
CREATE TABLE image_cache(
 source_hash TEXT NOT NULL, model TEXT NOT NULL, version INTEGER NOT NULL,
 extracted_text TEXT NOT NULL, description TEXT NOT NULL,
 PRIMARY KEY(source_hash,model,version)
);
CREATE TABLE image_usage(day INTEGER PRIMARY KEY, requests INTEGER NOT NULL);
