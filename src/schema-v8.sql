CREATE TABLE file_times(file_id INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE, modified INTEGER);
CREATE VIEW result_dates AS
SELECT f.id AS file_id,
 CASE WHEN r.content_format='slack' THEN CAST(r.remote_updated AS INTEGER)
      WHEN r.file_id IS NOT NULL THEN unixepoch(r.remote_updated)
      ELSE t.modified END AS timestamp
FROM files f LEFT JOIN attachments a ON a.file_id=f.id
LEFT JOIN remote_items r ON r.file_id=coalesce(a.parent_file_id,f.id)
LEFT JOIN file_times t ON t.file_id=f.id;
