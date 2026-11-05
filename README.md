# Good Search

A Windows 11 filename and text search launcher in Rust, SQLite, and egui/eframe. The window is rendered with OpenGL; there is no webview or local web server. This is the first working slice, not an installer or a Windows Search replacement.

## Run

Launch `target\release\good-search.exe`. On first launch, enter the folders to index in Settings and choose **Save changes**. Nothing is indexed until you select folders.

- **Ctrl+Alt+Space** shows and focuses the search window globally.
- Opening Good Search again reopens its existing window, including when hidden. Only one instance runs per data directory.
- The borderless window keeps results full width, with Settings and key hints in the footer. Drag the search icon to move the window.
- Search terms are combined with AND: `project budget` finds both, in either order. Quotes group phrases: `"project budget" approval` requires the phrase plus approval. An unfinished quote extends to the end of input. Content terms must occur in the same chunk; filename terms are literal substrings. A single one/two-character filename term still uses prefix matching. Matching is case insensitive; content phrases follow token boundaries rather than exact punctuation. Other operators and wildcard syntax are treated literally.
- While the current query is pending, the window shows **Searching…**. **No results** appears only after that query completes. Clearing the search clears results immediately.
- Ctrl+C, Ctrl+X, and Ctrl+V use the Windows clipboard in Search, Selected folders, and Exclusions.
- Up/down select a result; Enter or double-click opens it with its Windows default application.
- Escape or Alt+F4 hides the window. **Settings → Quit Good Search** exits the process.
- If another program owns the hotkey, the app displays an error and keeps normal close behavior so it remains usable.
- Escape still hides the window when the hotkey is unavailable; launch Good Search again to recover the hidden window.

Settings accept one absolute folder per line, including `C:\Users\you\Documents` and `\\wsl.localhost\Ubuntu-24.04\home\you\notes`. Use the distribution name shown by `wsl --list --quiet`.

Exclusions are globs matched against each basename and slash-separated path relative to a selected folder. Examples: `.git`, `node_modules`, `*.tmp`, `private/**`. Matching is case insensitive, including WSL. Excluded directories are pruned. Symbolic links and junctions are not followed. Paths with invalid Unicode produce a scan error rather than silently corrupting filenames.

The configuration and index live in `%LOCALAPPDATA%\GoodSearch` on Windows. `GOOD_SEARCH_DATA` can override that with an absolute local Windows directory for isolated tests. The database directory is excluded from indexing. Text indexing is opt-in under Settings / Search text inside files for each selected folder. It supports UTF-8 `.md`, `.markdown`, and `.txt`, with a configurable 2 MiB default size cap. Enabled content is stored locally in SQLite. Filename matches appear first, followed by text matches with excerpts. No API credentials or network calls are used.

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
2. OCR: add an opt-in, bounded image extraction queue and store recognized text locally. Track extractor version and failures for incremental retries.
3. Image descriptions: explicitly opt selected folders into OpenAI processing; only the indexing worker sends images. Cache descriptions with image hashes, model/prompt versions, retry state, and a cost budget. Store secrets in Windows Credential Manager.
4. Semantic image retrieval: descriptions alone give lexical search. Add local embeddings of descriptions and a local query embedding model/vector index for genuinely semantic retrieval without network requests at search time.

OCR, descriptions and embeddings are not implemented. The initial app performs no OpenAI or other network calls during indexing or searching.

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
