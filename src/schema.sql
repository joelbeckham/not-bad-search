-- Migration 1 also adopts the original unversioned filename index.
CREATE TABLE IF NOT EXISTS files(id INTEGER PRIMARY KEY, root TEXT NOT NULL, path TEXT NOT NULL UNIQUE, name TEXT NOT NULL, folded TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS files_prefix ON files(folded COLLATE NOCASE);
CREATE VIRTUAL TABLE IF NOT EXISTS names USING fts5(folded, content='files', content_rowid='id', tokenize='trigram');
CREATE TRIGGER IF NOT EXISTS files_ai AFTER INSERT ON files BEGIN INSERT INTO names(rowid,folded) VALUES(new.id,new.folded); END;
CREATE TRIGGER IF NOT EXISTS files_ad AFTER DELETE ON files BEGIN INSERT INTO names(names,rowid,folded) VALUES('delete',old.id,old.folded); END;
CREATE TRIGGER IF NOT EXISTS files_au AFTER UPDATE ON files BEGIN INSERT INTO names(names,rowid,folded) VALUES('delete',old.id,old.folded); INSERT INTO names(rowid,folded) VALUES(new.id,new.folded); END;

CREATE TABLE documents(
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 file_id INTEGER NOT NULL UNIQUE REFERENCES files(id) ON DELETE CASCADE,
 source_hash TEXT NOT NULL,
 extractor_version INTEGER NOT NULL,
 chunker_version INTEGER NOT NULL,
 status TEXT NOT NULL CHECK(status IN ('ready','skipped','error')),
 detail TEXT NOT NULL
);
CREATE TABLE chunks(
 id INTEGER PRIMARY KEY AUTOINCREMENT,
 document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
 ordinal INTEGER NOT NULL,
 heading TEXT NOT NULL,
 start_line INTEGER NOT NULL,
 body TEXT NOT NULL,
 content_hash TEXT NOT NULL,
 UNIQUE(document_id, ordinal)
);
CREATE VIRTUAL TABLE chunk_search USING fts5(heading,body,content='chunks',content_rowid='id',tokenize='unicode61');
CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN INSERT INTO chunk_search(rowid,heading,body) VALUES(new.id,new.heading,new.body); END;
CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN INSERT INTO chunk_search(chunk_search,rowid,heading,body) VALUES('delete',old.id,old.heading,old.body); END;
CREATE TRIGGER chunks_au AFTER UPDATE ON chunks BEGIN INSERT INTO chunk_search(chunk_search,rowid,heading,body) VALUES('delete',old.id,old.heading,old.body); INSERT INTO chunk_search(rowid,heading,body) VALUES(new.id,new.heading,new.body); END;
