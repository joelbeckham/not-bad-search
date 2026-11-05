# Validation — 2026-11-04

Performed on Windows x64 using project-local .NET SDK 10.0.403.

- Release extension build: passed, zero warnings and errors.
- Self-contained x64 publish and package asset generation: passed.
- Real named-pipe core tests: passed. Includes fragmented Unicode, escaped request text, preview fields, request ID/version validation, structured errors, malformed JSON, premature EOF, oversized responses, absent-service timeout, cancellation, and literal preview rendering.
- SDK-backed page tests: passed. Includes latest-query result selection with a backend that ignores cancellation, canceled preview lifetime, reentrant item notifications, snippet display, lazy preview reuse, clearing results, and backend errors.
- Published COM host startup: passed. Launched hidden with `-RegisterProcessAsComServer`, verified it remained alive, then terminated only that test process. No package registration was performed.
- Package manifest XML and package identity inspected; required generated assets are present.

Not yet verified: actual Command Palette package registration/discovery and visual rendering, interaction with the real Rust IPC backend, ARM64 build/runtime, signed MSIX distribution. The Rust IPC backend does not exist in the inspected source; until it implements IPC-CONTRACT.md, searches display the unavailable-backend state.

All authored project changes are under `extensions/GoodSearch.CommandPalette`. No Rust source or Cargo files were edited, and no SQLite database was read by this extension or its tests.
