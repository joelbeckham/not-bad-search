# Good Search for PowerToys Command Palette

C# extension backed only by the Rust service's named-pipe API. The Rust service must implement [IPC-CONTRACT.md](IPC-CONTRACT.md). Until then, the extension builds but shows a backend-unavailable message for searches. There is no SQLite fallback.

## Build

Requires Windows 11, PowerToys Command Palette, and .NET 10 SDK. The project pins the official Command Palette SDK `0.11.260520004`. A local SDK can be installed into the repository's ignored `.tools` directory:

```powershell
powershell -ExecutionPolicy Bypass -File extensions\GoodSearch.CommandPalette\bootstrap.ps1
powershell -ExecutionPolicy Bypass -File extensions\GoodSearch.CommandPalette\build.ps1
```

The build creates a self-contained x64 package layout under `AppPackages\x64`; use `-Architecture ARM64` for ARM64. Build outputs, `bin`, `obj`, and package layouts are ignored within this extension directory.

## Register locally

Enable Windows Developer Mode for loose package registration, then run:

```powershell
powershell -ExecutionPolicy Bypass -File extensions\GoodSearch.CommandPalette\build.ps1 -Register
```

This registers the built package for the current user. Keep the output directory in place while registered. Run **Reload** in Command Palette, select **Good Search**, and type a query. Up/down changes selection, Enter opens the file; the action menu includes **Show in File Explorer** and **Copy path**. The details pane requests a backend preview when the host reads the result's details. The host may prefetch details; requests are limited to two at once, retained per result, and canceled when the query changes.

To unregister:

```powershell
Get-AppxPackage -Name GoodSearch.CommandPalette | Remove-AppxPackage
```

The script creates development package assets and an unpackaged publish layout; it does not produce a signed distributable MSIX or list the extension in the gallery.

## Configuration

Defaults: pipe `good-search-v1`, total request timeout 5 seconds. Optional `%LOCALAPPDATA%\GoodSearch\command-palette.json`:

```json
{"pipe_name":"good-search-v1","timeout_seconds":5}
```

Reload the extension after changing this file. Rust owns indexing folders and exclusions; the extension never writes the Rust TOML config. It does not launch the backend, so start that separately. Search is debounced 120 ms, returns up to 100 results, and ignores canceled/stale responses. No query/content logging is added.

## Preview bounds

Plain-text excerpts render as literal text with backend-supplied line numbers. Optional snippets appear under result titles. Image previews use a backend-supplied local thumbnail. Text preview is requested at 64 KiB with three context lines. Content matching and extraction remain backend responsibilities. Custom match-color highlighting, PDF/Office rendering, Peek integration, and home-page result blending are not implemented.

## Tests

```powershell
.\.tools\dotnet\dotnet.exe run --project extensions\GoodSearch.CommandPalette\Tests\GoodSearch.Core.Tests.csproj
.\.tools\dotnet\dotnet.exe run --project extensions\GoodSearch.CommandPalette\AppTests\GoodSearch.App.Tests.csproj -p:Platform=x64
```

Tests use temporary current-user named pipes; no Rust process or index is needed. They cover real pipe framing, fragmented Unicode, request/response shapes, structured errors, oversized/malformed responses, premature disconnects, timeouts, cancellation, and literal preview rendering.

The Windows SDK-backed page tests also exercise stale-result rejection, query/preview cancellation, reentrant item notifications, lazy preview reuse, snippets, empty queries, and visible backend errors. Windows metadata for the build comes from a pinned NuGet dependency; a system-wide Windows SDK installation is not required.
