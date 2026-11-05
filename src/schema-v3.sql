CREATE TABLE embeddings(
 chunk_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
 model TEXT NOT NULL,
 content_hash TEXT NOT NULL,
 dimensions INTEGER NOT NULL,
 vector BLOB NOT NULL,
 PRIMARY KEY(chunk_id,model),
 CHECK(dimensions>0 AND length(vector)=dimensions*4)
);
CREATE TRIGGER embeddings_chunk_update AFTER UPDATE ON chunks BEGIN
 DELETE FROM embeddings WHERE chunk_id=new.id;
END;
