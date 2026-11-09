# Good Search

A Windows 11 filename and text search launcher in Rust, SQLite, and egui/eframe. The window is rendered with OpenGL; there is no webview or local web server. This is the first working slice, not an installer or a Windows Search replacement.

## Run

Jira summaries and descriptions can also be imported through Bifrost with a scoped JQL source. See [Jira setup, synchronization and limitations](docs/jira-source.md). Remote searches and previews use the local index. Gmail conversations can also be imported through Bifrost; see [Gmail setup and limitations](docs/gmail-source.md).

Launch `target\release\good-search.exe`. On first launch, enter the folders to index in Settings and choose **Save changes**. Nothing is indexed until you select folders.

- **Ctrl+Alt+Space** shows and focuses the search window globally.
- Opening Good Search again reopens its existing window, including when hidden. Only one instance runs per data directory.
- The borderless window keeps results full width, with Settings and key hints in the footer. The window centers in the mouse pointer monitor's work area on startup, hotkey activation, and reopening an existing instance.
- Search terms are combined with AND: `project budget` finds both, in either order. Quotes group phrases: `"project budget" approval` requires the phrase plus approval. An unfinished quote extends to the end of input. Content terms must occur in the same chunk; filename terms are literal substrings. A single one/two-character filename term still uses prefix matching. Matching is case insensitive; content phrases follow token boundaries rather than exact punctuation. Other operators and wildcard syntax are treated literally.
- While the current query is pending, the window shows **Searching...**. **No results** appears only after that query completes. Clearing the search clears results immediately.
- Ctrl+C, Ctrl+X, and Ctrl+V use the Windows clipboard in Search, Selected folders, and Exclusions.
- Up/down select a result. Enter or a click opens a full-window preview; Ctrl+Enter opens the file directly. In preview, Up/Down browse results, Enter opens the current file in its default application, and Escape returns to the list. The scroll wheel scrolls preview content.
- Ctrl+, opens Settings from search or Indexed data. Back/Escape returns to the previous screen.
- Escape returns from Settings, Indexed data or Preview to the previous screen, clears a nonempty search, or hides an empty search window. Alt+F4 hides the window. **Settings > Quit Good Search** exits the process.
- If another program owns the hotkey, the app displays an error and keeps normal close behavior so it remains usable.
- Escape on an empty search still hides the window when the hotkey is unavailable; launch Good Search again to recover the hidden window.

Settings accept one absolute folder per line, including `C:\Users\you\Documents` and `\\wsl.localhost\Ubuntu-24.04\home\you\notes`. Use the distribution name shown by `wsl --list --quiet`.

Exclusions are globs matched against each basename and slash-separated path relative to a selected folder. Examples: `.git`, `node_modules`, `*.tmp`, `private/**`. Matching is case insensitive, including WSL. Excluded directories are pruned. Symbolic links and junctions are not followed. Paths with invalid Unicode produce a scan error rather than silently corrupting filenames.

The configuration and index live in `%LOCALAPPDATA%\GoodSearch` on Windows. `GOOD_SEARCH_DATA` can override that with an absolute local Windows directory for isolated tests. The database directory is excluded from indexing. Text indexing is opt-in under Settings / Search text inside files for each selected folder. It supports UTF-8 `.md`, `.markdown`, and `.txt`, with a configurable 2 MiB default size cap. Enabled content is stored locally in SQLite. Filename matches appear first, followed by text matches with excerpts. Text-file indexing and all searches remain local. Optional image indexing sends opted-in images to OpenAI; credentials live in Windows Credential Manager.

## Build and test on Windows

The development toolchain used here is Rust 1.99.1, target `x86_64-pc-windows-gnu`, with portable w64devkit 2.11.0. Both are installed under `.tools` in this checkout. No WSL compilation is used.

```powershell
. .\scripts\env.ps1
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release
.\target\release\good-search.exe
```

For a new checkout, install Rust for Windows with a C linker (MSVC Build Tools or GNU MinGW). `scripts/env.ps1` is specifically for the project-local GNU toolchain; it enables Rust's self-contained runtime libraries. The app bundles SQLite; users do not install SQLite or Rust to run the executable.

Integration diagnostics require an interactive Windows desktop. They launch an isolated test instance, send real keyboard input, then stop only that instance. Avoid typing during the test. Close other Good Search instances first.

```powershell
powershell -ExecutionPolicy Bypass -File scripts\ui-smoke.ps1
powershell -STA -ExecutionPolicy Bypass -File scripts\clipboard-smoke.ps1 -Exe .\target\release\good-search.exe
powershell -STA -ExecutionPolicy Bypass -File scripts\search-repeat-smoke.ps1 -Rounds 100
powershell -ExecutionPolicy Bypass -File scripts\watch-smoke.ps1 -Distro Ubuntu-24.04
.\target\release\diagnostics.exe bench test-data\new-benchmark.sqlite
.\target\release\diagnostics.exe index test-data\folder.sqlite C:\path\to\fixture
```

The benchmark requires a new database filename. UI diagnostics log query text only when `GOOD_SEARCH_TRACE` is explicitly set, into the isolated data directory. The smoke script enables this temporarily. Normal app use has no query log.

`GOOD_SEARCH_WORKER_TRACE` separately enables detailed query submission/completion/acceptance logging for diagnosing timing issues. Leave it unset during latency tests: additional disk writes can affect the race being investigated.

## Architecture and current bounds

See the [SQLite schema reference](docs/database-schema.md) for columns, constraints, indexes, FTS triggers, reconciliation transactions, connection settings, and schema migrations and embedding preparation.

`src/lib.rs` is the independently testable indexing/search core. The GUI owns no SQLite connection. A dedicated query worker coalesces pending input and returns generation-tagged results, so stale results cannot overwrite newer input. A separate indexing thread enters Windows background scheduling mode, which lowers CPU, I/O and memory priority. SQLite WAL permits readers during index transactions.

Each selected root is scanned outside a transaction, then its filename snapshot is published atomically. Content extraction and hashing also occur outside transactions; changed chunks commit per document. Deleted files are removed only after a complete successful scan. An inaccessible folder retains its last successful snapshot and displays an error; results from that folder can be stale. Removing a selected root removes its indexed rows. Saving settings during a scan queues the new configuration for the next scan.

FTS5 trigram indexing handles substring searches. Short-prefix searches use a B-tree. Broad substring queries rank at most the first 1,000 matches and return up to 100 results; ranking is not exhaustive across all matches. Search executes entirely against the Windows-local SQLite database.

Windows `notify` watchers wake the indexer, with a 250 ms debounce. Missed events and offline folders are reconciled every 30 seconds by default (`reconcile_seconds` in `config.toml`, minimum 5). This version rescans selected roots rather than maintaining a USN journal or applying per-file events. Large trees and WSL mounts can therefore produce noticeable background I/O each cycle; the interval is configurable. Shutdown during a scan relies on SQLite transaction recovery.

**WSL notifications were tested, not assumed:** on this Windows/Ubuntu setup the watcher registered but received no events for Linux-side create/rename/delete. Periodic scanning successfully detected the same additions and deletions. Expect WSL updates within one reconciliation interval plus scan time. See [validation.md](validation.md) for machine-specific evidence.

egui provides compact Rust UI code and direct rendering, but uses custom widgets rather than native WinUI controls. Screen-reader support, IME behavior, high-DPI/multi-monitor placement, and GPU compatibility need broader validation before distribution. This build has no tray icon, installer, auto-start registration, or configurable hotkey.

## Later stages

1. Text search is implemented with versioned documents, chunks and FTS5. Current limits: raw Markdown, token phrases within chunks, and no PDF/Office extraction. Files are rehashed during reconciliation; unchanged chunks retain their IDs.
2. Image text and descriptions: implemented through one OpenAI request per uncached image revision; local OCR is intentionally omitted.
3. Image processing uses a persistent queue and hash/model/version cache, a daily request cap, pause/retry controls, and Windows Credential Manager.
4. Semantic retrieval uses local Potion 8M embeddings of text, extracted image text and image descriptions, with a debounced hybrid ranking pass.

Embeddings are not implemented. Only the opted-in image worker makes OpenAI calls; search never does.

## References

- [egui/eframe rendering and source](https://github.com/emilk/egui)
- [notify filesystem caveats](https://docs.rs/notify/latest/notify/)
- [Microsoft ReadDirectoryChangesW behavior](https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-readdirectorychangesw)
- [eframe event loop source](https://github.com/emilk/egui/blob/main/crates/eframe/src/native/run.rs)

To exercise content search with native keyboard input on an idle Windows desktop:

```powershell
powershell -STA -ExecutionPolicy Bypass -File scripts\search-repeat-smoke.ps1 -Rounds 20 -Content
```

The script creates isolated Markdown fixtures, verifies repeated content search and live text edits, and checks Escape/reopening while measuring process memory and idle CPU. It refuses keyboard input if the test window loses focus.

## Image indexing

In Settings, select image folders under **Search inside images with OpenAI**, store an API key, choose a model and daily request cap, then **Save changes**. Image opt-ins are separate from text-file opt-ins. PNG, JPEG and WebP are supported, up to 10 MiB and 20 megapixels per image. The model defaults to `gpt-6.1-terra`. The Image analysis model selector offers GPT-6.1 Terra and GPT-6 Luna. Select one, then Save changes. Existing explicitly saved model choices are preserved; changing the model reanalyzes selected images within the daily request cap. Availability depends on your OpenAI project.

One request returns faithful extracted text and a separate visual description. Results are labeled **Image text** or **Image description**. Both use the existing local AND/phrase search. Images themselves are not stored in SQLite; cached extracted text and descriptions are.

The daily cap counts attempted requests (including retries), resets at midnight UTC, and persists across restarts. It is not a dollar budget: cost depends on the selected model, image dimensions and output. Each call limits output to 8,192 tokens. Incomplete/refused responses are not indexed. One low-priority image worker runs independently of file scanning, with a 120-second request timeout and at most three attempts per job, separated by backoff. Interrupted requests require manual retry because a completed remote request may already have incurred a charge.

Pause takes effect after saving; an already-sent request may finish and be cached. Removing a folder opt-in removes its indexed image documents and unreferenced cache entries on the next scan. Deletion, exclusions, changed source bytes and changed model/prompt version invalidate image outputs. Duplicate images share cached extraction when hash/model/version match. Scans rehash eligible files, but unchanged images do not trigger new API calls.

The API key is stored as the current Windows user's generic credential `GoodSearch/OpenAI`, shared by isolated Good Search profiles. It is never written to TOML or SQLite. **Remove key** removes that credential. The app requests `store:false` from the Responses API; this is not a claim of zero provider retention. Folder opt-in authorizes uploading image bytes, including embedded metadata. OCR uses OpenAI; embeddings and searches run locally.

For a synthetic live API check only (one request maximum): create an image containing the word `marmalade`, then run `diagnostics image-test NEW_DB FIXTURE_FOLDER`. `diagnostics store-key-file PRIVATE_FILE` can import a key without placing its value in command arguments; the app's masked Settings field is the normal setup flow.

Right-click a search result for **Inspect indexed data**, **Copy path**, or **Open file**. The inspector fills the main content area like Settings, with Back to search in the footer. It displays a read-only local snapshot: all stored file fields, each extracted document and its chunks, image job metadata/errors, and the matching cached transcription/description. Expand sections to read full text; select/copy text or use **Copy all as JSON**. **Refresh** reloads the snapshot without reindexing or contacting OpenAI. Escape returns from the inspector to the previous screen, preserving the search. A removed file produces an explanatory message.

## File previews

Previews read the current file locally in one background worker; no OpenAI calls are made. PNG, JPEG and WebP images fit the window (up to 32 MiB / 40 megapixels, downscaled to 1600 pixels for display). UTF-8 text and raw Markdown show line numbers and highlighted search terms, initially scrolled near the first match. Text previews are limited to 2 MiB, 20,000 lines and 8 KiB per line; partial previews are labeled. Unsupported, missing or unreadable files show an explanation and can still be opened in their default application. Up/Down browse a snapshot of the results so background indexing cannot shift the preview order. Ctrl+, opens Settings and Back/Escape returns to the preview.

## Semantic search

Enabled by default. Filename and FTS results appear immediately; after a 200 ms typing pause, a second local pass retrieves related chunks and blends rankings using reciprocal-rank fusion. Settings lets you disable semantic search or change the delay (50–2000 ms). Queries shorter than three characters remain lexical. Quoted queries keep their strict lexical candidate set. Exact filename matches remain first; semantic-only results are labeled in their snippets. The selected file is preserved when rankings update, and preview navigation uses its existing result snapshot.

The MIT-licensed [Potion 8M model](https://huggingface.co/minishlab/potion-base-8M) runs via [Model2Vec Rust](https://github.com/MinishLab/model2vec-rs). A one-time download of about 30 MiB is stored under the Windows app data `models` directory, pinned to revision `bf8b056651a2c21b8d2565580b8569da283cab23`. No file content or search text is sent for embeddings. No API key, GPU, Python, webview or external service is needed for semantic search. The compact English model prioritizes speed; it is not a full language model and is weaker at nuanced meaning and non-English text.

A low-priority worker embeds ready chunks in batches, outside write transactions, and stores 256-dimensional normalized vectors in SQLite. Files without extracted content still have filename search. Changed/deleted chunks invalidate vectors automatically. Queries use a warm in-memory vector snapshot, exact cosine similarity (minimum 0.35), the best chunk per file, and a bounded cache of 64 query vectors. Snapshot refresh follows SQLite changes, and final result hydration checks current chunk hashes. Stale query generations are ignored. If model setup or semantic search fails, lexical search remains available; Settings shows the error. Disabling semantic search stops embedding work and releases the model/cache but retains vectors on disk for reuse.

Run `embedding-bench MODEL_FOLDER NEW_DATABASE` for deterministic relevance checks and a 10,000-chunk Windows benchmark. `scripts/semantic-ui-smoke.ps1` checks native debounce, repeated/replaced queries, quotes, preview navigation, clearing, memory and hidden idle CPU with synthetic local fixtures.

Three multiselect menus below the search box control results:

- **Sources:** select entire Folders/Jira/Slack/Gmail groups or individual sources. **All sources** is dynamic and automatically includes newly added sources. Paused sources remain searchable using their existing index.
- **Content:** **Text** includes text items from any source, including Jira titles/descriptions and local Markdown. **Image** includes image filenames and extracted image text/descriptions. Non-image filenames are categorized as Text; a Jira title ending in an image extension remains Text.
- **Matching:** **Keyword** includes names/titles and literal content matches; **Semantic** uses local embeddings. Semantic is disabled with an explanation when unavailable; there is no forced fallback to Keyword.

Options within a menu combine with OR, and menus combine with AND. All options start selected. Empty selections are allowed and produce no results. **Reset filters** restores dynamic All sources and both content/matching options. Selections survive hiding/reopening the window but reset on application restart. Source choices use stable source IDs, so renaming a source or editing its path preserves the selection. Changing a filter reruns the query and invalidates pending results; source/content restrictions apply before ranking limits in both search paths. Filters never change indexing, upload settings or pause state. Escape closes an open menu before clearing or hiding search.

Search results use bundled Catppuccin Mocha file icons (MIT): 390 icons with upstream filename and extension associations. Exact names take precedence over compound extensions; matching is case-insensitive and unknown files use a document outline. See assets/catppuccin/README.md for provenance and regeneration. Icons render natively and are cached locally.

Settings has Sources, Search, and Processing tabs. Every selected folder becomes a named source with a persistent ID, source-specific exclusions, text/image opt-ins, and pause state. Existing folder settings migrate automatically in memory and are persisted on Save changes. Sources shows file counts from the local index, separating filename searchability from readiness across enabled processing stages. Status reads run in a background thread only while Sources is open. Escape returns from a source detail view to the source list, then to the previous screen. Removing a source and saving removes its indexed records without deleting original files. Jira, Slack, Gmail and Notion are available through Bifrost. See [Notion setup, recursive scope and image indexing](docs/notion-source.md).

Slack indexes each opening message and its replies as one chronological conversation. Results show reply count and latest message time; previews include the conversation with matching terms highlighted. Long conversations use overlapping passages with opening-message context for semantic search. Images from all replies remain separately searchable and open the parent thread. Existing Slack sources automatically recheck their configured history to replace individual reply records with conversations. The update lookback still applies to the opening message's date: use **Recheck full history on next save** to discover replies added to older threads. Slack's connector does not expose reliable edit timestamps, so the displayed latest time means the most recent message, not the last edit.
