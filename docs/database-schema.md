# SQLite schema

Executable definitions: [schema.sql](../src/schema.sql). Migrations and connection setup: [storage.rs](../src/storage.rs). Extraction: [content.rs](../src/content.rs).

## Location and migration

The Windows-local database is `%LOCALAPPDATA%\GoodSearch\index.sqlite`. `GOOD_SEARCH_DATA` overrides the directory. WSL UNC paths are sources only. Configuration lives in `config.toml` beside the index.

`PRAGMA user_version = 1` identifies this schema. Startup adopts the original unversioned filename index (version 0) and creates content tables in one transaction, preserving filename rows. Workers start after migration. Newer versions are rejected. Future changes require numbered migrations.

## Tables and columns

All columns are NOT NULL, except primary keys which carry identity constraints.

| Table | Columns |
| --- | --- |
| `files` | `id INTEGER PRIMARY KEY`; `root TEXT`; `path TEXT UNIQUE`; `name TEXT`; `folded TEXT` |
| `documents` | `id INTEGER PRIMARY KEY AUTOINCREMENT`; `file_id INTEGER UNIQUE REFERENCES files(id) ON DELETE CASCADE`; `source_hash TEXT`; `extractor_version INTEGER`; `chunker_version INTEGER`; `status TEXT`; `detail TEXT` |
| `chunks` | `id INTEGER PRIMARY KEY AUTOINCREMENT`; `document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE`; `ordinal INTEGER`; `heading TEXT`; `start_line INTEGER`; `body TEXT`; `content_hash TEXT` |

`files.root` is the selected root, `path` the full Unicode path, `name` the basename, and `folded` its Rust lowercase form (not full Unicode case folding). `files_prefix` indexes `folded COLLATE NOCASE`. Path uniqueness uses default SQLite collation: Windows aliases/case variants are not canonicalized. Overlapping roots share one file record; the later scan assigns the root, which determines text opt-in.

`documents.source_hash` is the BLAKE3 hex digest of original bytes, or empty on failure. Algorithm versions are currently 1. Status is constrained to ready/skipped/error; this extractor uses ready/error. Detail is empty on success, otherwise a failure reason. Empty valid files have zero chunks. Read/size/encoding errors remove stale chunks and retry on subsequent scans. Disconnected roots retain their prior snapshot.

Chunks have zero-based ordinal, latest heuristic Markdown heading, one-based source start line, stored UTF-8 body and BLAKE3 body hash. `UNIQUE(document_id,ordinal)` indexes document membership.

IDs are internal, not durable filesystem identities. Rename currently means deletion plus insertion; IDs can change. Unchanged hashes and algorithm versions preserve chunk IDs. Changed documents replace chunks atomically. AUTOINCREMENT prevents reuse of deleted document/chunk IDs. Rebuilding the database can change all IDs.

## FTS and search

- `names`: FTS5 trigram over `files.folded`, external content `files`, row ID `id`.
- `chunk_search`: FTS5 unicode61 over `chunks.heading,body`, external content `chunks`, row ID `id`.
- `files_ai/ad/au` and `chunks_ai/ad/au` synchronize inserts/deletes/updates. Delete triggers use the external-content FTS delete command with old values.
- SQLite creates internal `names_*` and `chunk_search_*` shadow tables; do not edit them directly.

Filename matches come first. One/two-character queries use escaped prefix LIKE; longer queries use literal trigram phrases with substring verification. At most 1,000 filename candidates are ranked. Unquoted terms are ANDed; double quotes group phrases (an unfinished quote extends through end of input). Filename terms are substrings, using available trigram terms to narrow candidates and verifying every term before the candidate cap; a single short term retains prefix matching. Multiple short-only filename terms require a scan. Content terms/phrases must match the same chunk and are ranked by FTS rank; at most 1,000 chunks are considered and deduplicated by file. Input is never interpreted as FTS operators. Core results cap at 200; UI requests 100. Excerpts use FTS snippet with bracketed matching tokens.

## Indexing and transactions

1. Validate roots, text opt-ins, exclusions and byte cap.
2. Fill temporary `roots(path TEXT PRIMARY KEY)` and delete deselected filename records in a transaction. Cascades/triggers remove content and FTS entries.
3. Traverse each root outside a transaction into an in-memory filename snapshot. Prune exclusions, symlinks/junctions and database directory. Failed walks publish no partial filename snapshot.
4. Publish a successful root atomically: fill temporary `seen(path TEXT PRIMARY KEY)`, upsert files, delete unseen paths. Unchanged names avoid FTS updates.
5. Read opted-in .md/.markdown/.txt outside transactions, capped at configured bytes plus one. Default 2 MiB; UI range 1–64 MiB. Require UTF-8 (optional BOM), reject NUL/binary input. Hash every reconciliation so same-size edits do not depend on timestamp precision.
6. Skip unchanged hash/version/status. Chunk changed text outside transactions; replace each document's chunks in one transaction. Opt-out deletes documents while retaining filenames.

Chunks are approximately 4 KiB, split at lines/headings when possible and Unicode boundaries for long lines. Markdown syntax and code remain searchable. Heading detection is heuristic, not a complete Markdown parser. Line endings are normalized. Phrases do not match across chunk boundaries. Displayed line numbers indicate chunk starts, not exact match locations.

Filename/content publication is separate; content becomes available progressively. Queries never read source files. Reconciliation still defaults to 30 seconds: hashing opted-in files adds background I/O, especially on WSL. Unchanged content avoids database writes; event-targeted reads/metadata caching are not implemented yet.

## Connections

- Startup open_db: WAL and migrations before workers start.
- Index open_writer: five-second busy timeout, synchronous=NORMAL.
- Search open_reader: SQLite read-only flags, query_only=ON, 100 ms busy timeout, no DDL.
- Every connection enables foreign_keys=ON.

WAL permits ordinary readers during writes, not multiple simultaneous writers or immunity from all busy conditions. Single-instance ownership is per canonical data directory. Large filename commits can take time; traversal/extraction hold no writer transaction.

The index.sqlite-wal and index.sqlite-shm files belong with the database. For an offline backup, quit the app and copy the data directory; do not copy only an actively written main file.

## Future embeddings

No vector tables, vector extension or embedding/API calls exist. Future vectors should reference chunk identity and content revision, with model/version/dimensions/metric metadata. Do not mix incompatible embedding spaces. Replacing chunks must invalidate vectors transactionally, or via durable synchronization for an external index. Local semantic queries also require local query embeddings.

## Read-only inspection

```sql
PRAGMA user_version;
PRAGMA journal_mode;
PRAGMA foreign_key_check;
SELECT type,name,sql FROM sqlite_schema WHERE name NOT LIKE 'sqlite_%';
SELECT root,count(*) FROM files GROUP BY root;
SELECT status,count(*) FROM documents GROUP BY status;
SELECT count(*) FROM chunks;
SELECT f.path,d.detail FROM documents d JOIN files f ON f.id=d.file_id WHERE d.status='error';
```

Connection-specific pragmas in an external client do not describe the running app's settings.