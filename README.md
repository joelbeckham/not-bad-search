# Good Search

A Windows 11 filename and text search launcher in Rust, SQLite, and egui/eframe. The window is rendered with OpenGL; there is no webview or local web server. This is the first working slice, not an installer or a Windows Search replacement.

## Run

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
4. Semantic image retrieval: descriptions alone give lexical search. Add local embeddings of descriptions and a local query embedding model/vector index for genuinely semantic retrieval without network requests at search time.

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

In Settings, select image folders under **Search inside images with OpenAI**, store an API key, choose a model and daily request cap, then **Save changes**. Image opt-ins are separate from text-file opt-ins. PNG, JPEG and WebP are supported, up to 10 MiB and 20 megapixels per image. The model defaults to `gpt-6.1-terra`. Use the Image analysis model selector in Settings or enter a custom API model ID, then Save changes. Existing explicitly saved model choices are preserved; changing the model reanalyzes selected images within the daily request cap. Availability depends on your OpenAI project.

One request returns faithful extracted text and a separate visual description. Results are labeled **Image text** or **Image description**. Both use the existing local AND/phrase search. Images themselves are not stored in SQLite; cached extracted text and descriptions are.

The daily cap counts attempted requests (including retries), resets at midnight UTC, and persists across restarts. It is not a dollar budget: cost depends on the selected model, image dimensions and output. Each call limits output to 8,192 tokens. Incomplete/refused responses are not indexed. One low-priority image worker runs independently of file scanning, with a 120-second request timeout and at most three attempts per job, separated by backoff. Interrupted requests require manual retry because a completed remote request may already have incurred a charge.

Pause takes effect after saving; an already-sent request may finish and be cached. Removing a folder opt-in removes its indexed image documents and unreferenced cache entries on the next scan. Deletion, exclusions, changed source bytes and changed model/prompt version invalidate image outputs. Duplicate images share cached extraction when hash/model/version match. Scans rehash eligible files, but unchanged images do not trigger new API calls.

The API key is stored as the current Windows user's generic credential `GoodSearch/OpenAI`, shared by isolated Good Search profiles. It is never written to TOML or SQLite. **Remove key** removes that credential. The app requests `store:false` from the Responses API; this is not a claim of zero provider retention. Folder opt-in authorizes uploading image bytes, including embedded metadata. No local OCR or embeddings are used.

For a synthetic live API check only (one request maximum): create an image containing the word `marmalade`, then run `diagnostics image-test NEW_DB FIXTURE_FOLDER`. `diagnostics store-key-file PRIVATE_FILE` can import a key without placing its value in command arguments; the app's masked Settings field is the normal setup flow.

Right-click a search result for **Inspect indexed data**, **Copy path**, or **Open file**. The inspector fills the main content area like Settings, with Back to search in the footer. It displays a read-only local snapshot: all stored file fields, each extracted document and its chunks, image job metadata/errors, and the matching cached transcription/description. Expand sections to read full text; select/copy text or use **Copy all as JSON**. **Refresh** reloads the snapshot without reindexing or contacting OpenAI. Escape returns from the inspector to the previous screen, preserving the search. A removed file produces an explanatory message.

## File previews

Previews read the current file locally in one background worker; no OpenAI calls are made. PNG, JPEG and WebP images fit the window (up to 32 MiB / 40 megapixels, downscaled to 1600 pixels for display). UTF-8 text and raw Markdown show line numbers and highlighted search terms, initially scrolled near the first match. Text previews are limited to 2 MiB, 20,000 lines and 8 KiB per line; partial previews are labeled. Unsupported, missing or unreadable files show an explanation and can still be opened in their default application. Up/Down browse a snapshot of the results so background indexing cannot shift the preview order. Ctrl+, opens Settings and Back/Escape returns to the preview.
