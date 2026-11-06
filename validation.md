# Windows validation — 2026-11-04

Environment: Windows 11 Business, build 26200, Intel Core Ultra 9 285H, 16 logical processors. Rust 1.99.1, Windows GNU target, w64devkit 2.11.0, bundled SQLite. All compilation and application execution occurred on Windows. Ubuntu 24.04 was used only as a source filesystem and to generate Linux-side changes.

## Dependency refresh

Instance/Escape follow-up: found an outdated hidden process (PID 38388, started 14:24:24) still running after binary replacements. Escape had been conditional on successful global-hotkey registration, so another process owning the hotkey disabled hiding. Escape now hides independently of registration. A Windows named mutex keyed to the canonical data directory plus a window property makes subsequent launches show the existing window instead of creating another copy. An old pre-guard process was retired during installation.

Search completion now checks both request generation and echoed query text, and retains a completed-generation marker. This prevents stale completions and settings invalidation from presenting an empty result as completion of a newer query. The earlier 16 ms pending-query follow-up remains. A separate test index over the actual configured `C:\tools\screenshots` folder passed 100 clear/retype cycles for `2026` with normal 30-second reconciliation enabled: mean 125.47 ms, maximum 135.44 ms including simulated typing. Escape while the hotkey was occupied, hiding, and reopening the same instance by launching the executable passed. The reported 10-second stall was not reproduced in this updated real-folder run; these results do not establish every possible prior scheduling cause.

After retiring the outdated process, the standard desktop test passed ten global-hotkey/focus cycles, Escape, typing, rename refresh, and close-to-hide. Hidden CPU was 0.268% of one core over 35 seconds. Core tests (five), strict Clippy, and the release build passed. Logs: `test-data/real-folder-search-validation.txt` and `test-data/instance-hotkey-validation.txt`. The production app was restarted with the saved configuration so the tested fixes are actually loaded.

Borderless UI redesign: visually inspected the native search, selection, empty, and settings views at Windows 125% scaling. The window has no visible title bar, rounded dark surfaces, a large search input, full-width filename/path rows, extension labels, and a fixed footer with settings and keyboard hints. Verified native dragging from the search icon and clipboard copy/cut/paste in all three text fields. Windows/winit retains some nonclient style bits internally; actual screenshots, rather than a WS_CAPTION bit check, establish that the title bar is absent.

Five core tests, strict Clippy, and the release build passed. Twenty clear/retype cycles passed with maximum simulated-input-to-result latency 134.59 ms. The final visible idle sample measured 3.59% of one core and hidden idle measured 0% over ten seconds each. An earlier visible sample reached 7.64% during desktop testing; visible pointer/caret/compositor activity is reported separately from the hidden polling-loop budget. The test now checks the hidden budget and reports both measurements. Native desktop test helpers use DPI-aware coordinates and focus checks so they avoid typing into another application.

Repeated-search follow-up: the old binary exceeded a 1.5-second UI-result deadline on the seventh `2026` / backspace-to-empty / `2026` cycle over a 2,279-file fixture. Detailed logging changed the timing and did not reproduce the stall consistently. Inspection identified a gap where a fast worker response can arrive after the frame's event drain, along with repeated UI passes that could resubmit the same value. The app now schedules a 16 ms follow-up only while a search is pending, submits each distinct input value once, handles empty queries synchronously, and tags query errors with their generation. Pending searches show `Searching…` instead of a false `No results` and the previous query's timing.

The final regression run, with detailed worker logging disabled, passed 100 clear-and-retype cycles and a genuine no-match query. Each response had to match a fresh generation, including the empty state between searches. Mean simulated-input-to-UI-result latency was 125.07 ms, maximum 136.19 ms; these include `SendKeys` typing and test polling. Settled visible idle CPU measured 0% of one core over ten seconds in that sample. Five core tests, strict Clippy, and the Windows release build passed. Raw output: `test-data/search-repeat-validation.txt`. This validates the tested sequence, not every possible desktop scheduling interleaving.

Clipboard follow-up: enabled eframe's explicit `clipboard` feature. With default features disabled, eframe 0.37 otherwise falls back to a private, in-app clipboard. A Windows regression test reproduced the failure in the old binary, then verified external clipboard paste, copy, cut, and paste-after-cut in Selected folders, Exclusions, and Search in the fixed release. The test uses an isolated instance, never saves the fixture paths, and restores the original clipboard data afterward. Strict Clippy and the Windows release build passed. The tested binary was installed at the normal launch path; already-running instances need to restart.

Updated direct dependencies to the latest published releases checked on 2026-11-04: rusqlite 0.41.1, toml 1.2.0+spec-1.1.0, and windows-sys 0.61.2. The remaining direct dependencies were already current. `cargo update` refreshed compatible transitive dependencies; upstream constraints still determine transitive version selection. Bundled SQLite is now 3.54.1. No application API changes were needed.

The refreshed build passed all five core tests, strict Clippy, formatting, and the Windows release build. It reopened and reconciled the existing 10,000-file index without errors (32.07 ms). A new 100,000-row benchmark completed ingestion in 4,817.99 ms; all six tested query p95 values remained below 5 ms (maximum 4.942 ms). Updated executable size: 10,635,776 bytes.

The desktop smoke test also passed: ten hotkey/focus cycles, Escape, live typing results, rename refresh, and close-to-hide. Hotkey samples ranged from 29.87 to 31.82 ms; typing-to-result was 389.94 ms and rename-to-result was 281.73 ms. Working set was 83.90 MiB, private memory 56.66 MiB. Visible idle CPU measured 1.404% of one logical core over ten seconds; hidden idle CPU measured 0.446% over 35 seconds. Raw output is in `test-data/ui-dependency-refresh.txt`. These are short, single-run samples subject to scheduling and desktop activity.

The sections below retain the original build's measurements for reference.

## Automated checks

`cargo test`: five tests passed. Coverage includes case-insensitive filename search, Unicode, literal punctuation and FTS query escaping, folder/file exclusions, changing exclusions, renames, deletion, removing a root, offline snapshot preservation, app data exclusion, and configuration validation (including WSL UNC syntax).

`cargo clippy --all-targets -- -D warnings`: passed. Release executable built successfully. The final renderer is egui/eframe 0.37 with Glow. Executable size: 10,739,712 bytes (10.24 MiB).

## Search and indexing

`diagnostics bench` populated 100,000 synthetic filename rows in an on-disk SQLite database. Insertion took 4,690 ms; the closed database was 31,449,088 bytes. This measures database ingestion, not crawling 100,000 filesystem entries.

Each query ran 100 times. Warm query p95 measurements were 0.515 ms for `report`, 4.384 ms for `project-042`, 0.127 ms for `000123`, 0.055 ms for prefix `pr`, 0.032 ms for `missing`, and 0.007 ms for prefix `md`. These include SQL execution and result allocation, not keyboard delivery or display presentation. Common substring queries rank a bounded set of 1,000 candidates; one/two-character queries use prefix semantics.

A separate real Windows directory with 10,000 empty Markdown files indexed in 144.71 ms; an unchanged rescan took 32.00 ms. These diagnostics run at normal priority. The app's index worker uses Windows background mode, so its timings may be slower under contention.

## Desktop integration

The reproducible `scripts/ui-smoke.ps1` uses an isolated config and real synthesized keyboard events. It checks global hotkey activation and foreground focus ten times, Escape hiding, typing a query and receiving its result in an egui UI pass, native rename propagation to the displayed results, and close-to-hide behavior. It samples visible idle CPU for ten seconds and hidden idle CPU for 35 seconds, including one periodic reconciliation.

The trace confirms UI submission, not the time when pixels reach the display. Typing latency includes Windows `SendKeys` delivery and the test's polling overhead. CPU percentages are relative to one logical core, not total system CPU. Memory and idle measurements use a tiny fixture, not a fully populated personal index.

Final run passed all desktop assertions. Ten hotkey samples ranged from 30.97 to 31.73 ms (median approximately 31.26 ms). Sending the 12-character query through `SendKeys` and observing its UI result took 404.41 ms. A native filename rename propagated to the displayed results in 264.63 ms.

Working set was 83.37 MiB, private memory 56.36 MiB. Visible idle CPU was 0.468% of one logical core over ten seconds; hidden idle CPU was 0.089% over 35 seconds. This hidden sample includes the configured 30-second periodic reconciliation. Raw output is retained locally in `test-data/ui-validation.txt`; test fixtures are excluded from version control.

## WSL change notifications

`scripts/watch-smoke.ps1` watched a fresh directory under `\\wsl.localhost\Ubuntu-24.04\tmp` with the same Rust `notify` Windows backend used by the app. After watcher readiness, `wsl.exe` created a file, renamed it, and deleted it from Linux. During the 20-second observation window, **zero notifications arrived**, even though watcher registration succeeded.

Windows-local SQLite reconciliation over that UNC path then found a Linux-created file (one indexed row, 4.13 ms), and removed it after Linux deletion (zero indexed rows, 4.57 ms). Thus scanning works on this machine, while Linux-side changes require the app's periodic reconciliation fallback. This result is specific to the installed Windows/WSL versions; it is not a claim about all WSL hosts.

## Findings and limits

The first egui evaluation used eframe 0.31. Its hidden-window event loop consumed approximately a full logical core. Source inspection identified the polling behavior; the app was upgraded to 0.37, which contains the event-loop correction. Hotkey focus now targets the native HWND directly rather than finding the window by title.

Broader tests remain for large real-world folder trees, inaccessible subdirectories, overlapping roots, long paths, WSL reconnects, multi-monitor/high-DPI placement, IME, accessibility, and different GPU drivers. Network mounts can block filesystem calls; those calls stay on the indexing thread. Periodic full scans are the main scaling limit of this first slice.

## Text indexing and embedding-ready storage (2026-11-04)

- Windows GNU debug tests: 8 passed (5 existing core tests, 3 content/migration integration tests).
- Strict Clippy over all targets: passed. Windows release binary built, SHA-256 `942350E1B41186EA0DF4AF37E7BE44F1EDFFF8556EBF41B30FFDB7E70E3613C0`.
- Verified legacy version-0 migration, filename retention, read-only searches during uncommitted writes, opt-in/out, edits, rename, exclusions, malformed/oversized files, Unicode, chunk version invalidation and FTS integrity.
- Native text search fixture: 2,279 Markdown files. One interrupted 100-round run completed 82 query/clear/retype cycles before losing foreground focus. SQL search samples: median 1.250 ms, p95 1.429 ms, maximum 1.583 ms. These measure database search only, not end-to-end input latency. This initial fixture included one matching filename; remaining results were content matches.
- A shorter run verified live text-edit notification refresh. App-only screenshot inspected: full-width rows with filename, path and content excerpt.
- Desktop tests were interrupted by focus changes and incomplete synthetic input. No complete new idle/hotkey measurement is claimed for those runs. Existing older validation below/above does not substitute for a fresh result.
- Existing local index/settings backed up before migration under `test-data/before-content-014fe5559b5646b4b322bddfe62803be`.

### Completed native hotkey validation (2026-11-04)

On the idle Windows desktop, `scripts/ui-smoke.ps1` passed 10 hotkey show/focus cycles, Escape hiding, filename search, native rename refresh, and close-to-hide. Hotkey latency was 15.87�43.79 ms. Typing the full synthetic filename query through SendKeys to its rendered result took 407.05 ms (includes typing); rename-to-result took 264.07 ms. Working set: 92.18 MiB; private memory: 56.67 MiB. Visible idle CPU: 0.625% of one core over 10 seconds; hidden idle CPU: 0.848% over 35 seconds, spanning periodic reconciliation. Fixture: `test-data/ui-86cf085d04ba4b38a7902ec8d1fd01ad`.

The user confirmed mouse activity during an interrupted content run; those incomplete runs do not establish an application stall.

### Completed native content validation (2026-11-04)

On the idle desktop, `scripts/search-repeat-smoke.ps1 -Rounds 20 -Content` passed all 20 search/clear/retype cycles against 2,279 text fixtures with no matching filenames. Maximum input-to-result latency: 138.15 ms; mean: 127.06 ms (includes SendKeys typing). Live text-edit notifications refreshed the result; Escape hid the window; launching a second instance reopened the existing one and exited. Working set: 98.93 MiB. Visible/hidden idle CPU: 0.312%/0.312% of one core over separate 10-second samples. Fixture: `test-data/repeat-1a3834ff9bf84ea1aabb75c7e8d54bce`. This completes the previously pending native checks; no application code changes were needed during this validation pass.

## AND terms and quoted phrases

Windows automated tests: 9 passed; strict Clippy passed. Integration coverage verifies unordered terms in both filenames and chunks, quoted phrases, mixed terms/phrases, multiple phrases, unfinished/empty quotes, literal OR and punctuation, and missing required terms. Existing filename, content lifecycle and migration tests remain green. No schema migration or reindex is required. Native input tests were not rerun for this query-only change.

## OpenAI image extraction and schema v2 (2026-11-04)

- 14 Windows automated tests passed; strict Clippy over all targets passed; Windows GNU release built.
- Migration test starts from a populated v1 database, preserves document/chunk identities and searchable FTS content, allows multiple source documents, and verifies cascading deletion plus FTS integrity.
- Fake-provider tests verify duplicate-image cache reuse, unchanged scan behavior, changed-byte invalidation, opt-out cleanup, pause/resume caching, persistent request caps, retry backoff, interrupted-request handling, stale-result rejection, and refusal/incomplete/malformed response rejection.
- Live API test: exactly one request using gpt-5.6-terra on a generated synthetic invoice image. Both extracted_text and description were published; searching marmalade returned the image. No user images were uploaded by this test. Credential was imported through a private file into Windows Credential Manager without exposing it in logs or arguments.
- Native image UI test used the already-indexed fixture with daily request cap=0: labeled image-text result, settings layout and Escape passed. Hidden CPU over 10 seconds: 0.156% of one core; working set: 100.51 MiB. App-only search/settings screenshots inspected. Fixture: test-data/image-api-0316de855c524943a523663549eadde2.
- These checks do not establish OCR accuracy across real-world images or WSL image throughput. Existing WSL notification limitations still apply. No local OCR or embeddings were added.

## Indexed-data inspector

15 automated Windows tests and strict Clippy passed. The inspector test verifies complete metadata, both image extraction kinds, multiline chunk bodies, hashes, job attempts, cached descriptions, read-only access and removed-file handling. Native synthetic-image test verified the context menu, inspector rendering, Escape closing the inspector without hiding the app, then normal Escape hiding. Screenshots inspected. Hidden idle CPU: 0.156% of one core over 10 seconds; working set: 93.63 MiB. Initial synthetic clicks were too short for reliable menu activation; the test now uses a normal-duration press/release. No extraction/API requests occur when inspecting or refreshing.

Full-page inspector update: Windows release build and strict Clippy passed. Native check verified full-window rendering, footer Back to search, reopening the inspector, and Escape returning to search without hiding the app. Hidden idle CPU: 0.156% of one core; working set: 99.80 MiB. Screenshot inspected.

Escape navigation update: strict Clippy and Windows release build passed. Native inspector smoke check passed: footer Back, inspector Escape, Settings Escape preserving the query, Escape clearing query/results while remaining visible, and a final Escape hiding the empty search. Screenshot verified. Hidden CPU 0.469% of one core, working set 100.02 MiB. Updated existing image/search smoke scripts for the new behavior.

Image model settings update: default gpt-6.1-terra, preset selector plus editable custom API ID. Explicit saved choices remain intact. All 15 automated Windows tests, strict Clippy and release build passed. Live synthetic invoice test with Terra 6.1 made one request and produced both documents plus a searchable transcription. Native Settings layout and Escape smoke passed using a pinned cached GPT-5.6 fixture with request cap zero; screenshot inspected. Hidden CPU 0.624% of one core; working set 94.98 MiB.

Ctrl+comma shortcut: Windows release and strict Clippy passed. Native check verified Settings from search and inspector, repeated Ctrl+comma remaining in Settings, and Escape returning to the inspector with its data intact. Screenshots inspected.

## Full-window file previews

17 automated Windows tests passed; strict Clippy and release build passed. Preview tests cover Unicode and quoted-term highlighting, partial UTF-8 boundaries, text/line limits, image resizing, binary/invalid/missing files and directories. Native preview smoke passed: Enter from search, clicking a filename, Up/Down, rapid navigation, Settings/Back, Escape preserving the list, and real default-application activation from both preview Enter and list Ctrl+Enter. Text wheel scrolling kept the same file; screenshots inspected. Existing inspector smoke passed with its new context-menu position. Synthetic 800x400 image input-to-ready was 28-31 ms; working set 106-114 MiB and hidden idle CPU 0-0.94% of one core across five-second samples. These are fixture measurements, not guarantees for large files or WSL reads. No API requests are made for previews; reads are bounded and stale worker replies are ignored.

## Window centering

Windows release build and strict Clippy passed. Native positioning test verified startup, the WM_HOTKEY handler while visible and hidden, and reopening the existing instance, each within 0.5 physical pixels of a monitor work-area center. Tests deliberately displaced the window before reactivation. Injected Ctrl+Alt+Space did not reach the test app, so the handler was checked with posted WM_HOTKEY messages; this run does not establish physical-key delivery. Monitor checks tolerate pointer movement during testing. Centering uses per-monitor DPI-aware physical coordinates and the pointer monitor's work area. Header dragging was removed.

## Local semantic search (2026-11-05)

- 21 automated Windows tests passed. New tests cover v2-to-v3 migration, embedding persistence, deduplication, change/delete invalidation, stale publication rejection, invalid vectors, exact-filename protection, strict quotes and hybrid ordering. After the final scoring changes, content/inspection/semantic tests passed again. Strict Clippy passed over all final targets.
- Potion 8M (pinned revision bf8b056651a2c21b8d2565580b8569da283cab23) passed four deterministic synonym fixtures. Local query embeddings: p95 0.002 ms; model load 44–88 ms across runs. Final 10,000-chunk benchmark: indexing 1479 ms, initial vector snapshot load 65 ms, query embedding plus semantic retrieval p50 2.654 ms / p95 3.261 ms / p99 3.813 ms. The index uses 256-dimensional vectors and exact scanning; these fixture measurements do not establish large-corpus or multilingual relevance.
- Native semantic UI smoke passed with no embedding network requests: immediate lexical results, semantic-only retrieval, repeated query, replacing a query before debounce, strict quotes, preview, clearing pending work, Settings and hiding. Screenshots inspected. Results arrived 198–205 ms after synthetic typing finished with a 200 ms debounce. The total first-keystroke-to-result time was 702–711 ms because SendKeys deliberately types characters over time. Hidden idle CPU 0.31% of one core over 10 seconds; working set 134.93 MiB. Test app hotkey registration conflicted with the normal instance, so global hotkey behavior was not retested here.
- Installed the verified build and pinned model locally. Pre-migration database/config/WAL backup: test-data/before-semantic-5c97adcb05f84d29b7caf868fe8e3a03. Existing filename, text and image extraction sources are retained; no re-upload to OpenAI is required for embedding them.

Post-install verification: the normal index contains 1440 / 1440 embedded ready chunks. Running app working set sampled at 125.29 MiB.

Search filter row: content and semantic integration tests passed (9 tests), including default-on options, text/image content separation before ranking, filename matches with both content types disabled, semantic-only and lexical-only modes. Strict Clippy and Windows release build passed. Native filter smoke clicked all four toggles, verified removal/restoration of semantic matches, and exercised the existing debounce, query replacement, preview, quote and clear flows. Screenshots inspected. Hidden CPU 0.312% of one core; working set 134.87 MiB. Filter values are session-only and each change invalidates prior query generations.

SQLite writer contention fix: Windows regression reproduces SQLITE_BUSY_SNAPSHOT with a deferred read/write transaction, verifies writer transactions serialize before their first read, and confirms search readers remain usable during a writer transaction. Writer connections now default to BEGIN IMMEDIATE; reader connections remain deferred/read-only. All 16 content/image/semantic/storage integration tests passed. Indexing error banners now clear after successful reconciliation without clearing unrelated errors.

Catppuccin icons: vendored v1.26.0 Mocha file assets and mappings with MIT license and package SHA-256. Windows unit tests verify filename precedence, uppercase/compound extensions, fallback, and rasterization of all 390 mapped SVGs. Strict Clippy and release build passed. Native isolated UI check displayed Markdown, Rust, PDF, PNG and ZIP results; screenshot visually inspected at test-data/icons-ui-05a69519956b4986aad4b68112e5fa25/file-icons.png. Fixture hotkey warning is expected while the normal app owns the global hotkey.

Background refresh flicker: native regression against the previous release reproduced an empty result list on the first indexing refresh of a semantic-only match. The corrected release passed three file-change refreshes without blanking, then correctly cleared the match after its source content changed. Existing selection is preserved by path when publishing results; changed queries and filters retain their normal immediate behavior. Semantic failure/unavailability publishes available lexical results instead of retaining an obsolete snapshot. Strict Clippy and Windows release build passed. Native traces: test-data/semantic-ui-def0ffbb32d048478284fd5e29a573cc.

Search input ownership: the results screen requests search focus before TextEdit handles input, so the first typed character or Backspace after another interaction is retained. Settings, preview and inspector screens keep their own focus behavior. Native Windows smoke passed typing/backspace after Down-arrow result navigation, a background click and wheel scroll, and a filter click (zero results for appended x, five restored by Backspace). Release build passed. Traces: test-data/icons-ui-79f0aaa9f290433f947b3b8435a60efe.

Source settings reorganization: Windows tests passed for legacy source adoption and stable IDs, per-source exclusions, pause/resume, index-only removal, per-file stage counts, paused image extraction and paused embeddings. Existing content (4), image (5), semantic (5), and source (4) integration tests passed; strict Clippy passed. Native isolated flow verified Sources/Search/Processing tabs, source detail and Escape navigation, saving legacy folders as sources with retained text opt-in/exclusions, and searching after returning from Settings. Layout screenshots inspected in test-data/icons-ui-fe9e5d8fe0e84423b547a52aa64617cd. Added low-priority background source snapshots while Sources is open; no remote integrations or database schema changes.
