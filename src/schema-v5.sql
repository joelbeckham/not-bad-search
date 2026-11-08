CREATE TABLE attachments (
 file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
 parent_file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
 connector TEXT NOT NULL, source_id TEXT NOT NULL, remote_id TEXT NOT NULL,
 filename TEXT NOT NULL, mime_type TEXT NOT NULL, byte_size INTEGER NOT NULL,
 content_hash TEXT NOT NULL, synced_at TEXT NOT NULL,
 UNIQUE(parent_file_id,remote_id)
);
CREATE INDEX attachments_parent ON attachments(parent_file_id);
CREATE TABLE attachment_scans (
 parent_file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE,
 remote_updated TEXT NOT NULL
);
CREATE TRIGGER remove_attachment_files BEFORE DELETE ON files BEGIN
 DELETE FROM files WHERE id IN (SELECT file_id FROM attachments WHERE parent_file_id=OLD.id);
END;
