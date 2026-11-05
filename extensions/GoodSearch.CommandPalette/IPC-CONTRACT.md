# Rust backend handoff: Good Search Command Palette IPC v1

The extension uses IPC exclusively. It does not open SQLite, scan folders, or read preview file contents. No Rust code is changed by the extension project.

## Transport

- Windows local named pipe: `\\.\pipe\good-search-v1` (client config contains just `good-search-v1`).
- Byte mode; UTF-8 without BOM; each message is one JSON object followed by LF. JSON string newlines must be escaped. CRLF responses are accepted.
- One request and one response per connection. The client closes after receiving the response. Handle multiple simultaneous connections (search plus up to two previews); never hold a global index/write lock while waiting for pipe I/O.
- Every request has integer `version: 1` and an opaque string `id`. Echo both in responses. Unknown fields should be ignored for forward compatibility.
- Maximum JSON frame: 1,048,576 bytes, excluding LF. Enforce a limit while reading, not after buffering arbitrary input. Default client deadline: 5 seconds for connect + response combined.
- Client cancellation closes its connection. Treat a disconnected client as a normal event; discard/cancel its work when practical. Indexing must survive canceled queries.
- Restrict server ACL to the current Windows user; reject remote clients. Backend and extension run under the same account, without elevation. Use a separate process/mutex so only one backend owns this pipe.
- Backend startup is external for v1. The extension does not launch executables or start indexing. It displays a retryable error when the backend is absent.

## Required operation: search

Request:

```json
{"version":1,"id":"search-1","op":"search","query":"report","limit":100}
```

Response:

```json
{"version":1,"id":"search-1","ok":true,"result":{"hits":[{"path":"C:\\notes\\report.md","name":"report.md","snippets":[{"line":42,"text":"Quarterly report results"}]}]}}
```

Initially wrap the existing Rust `search` function and return its exact order. `snippets` is optional and may be omitted or empty. `path` and `name` are required strings; paths must be absolute native Windows paths, including WSL UNC paths. Cap results at the requested limit (max 200 on the server); empty query may return an empty array. The extension requests 100 and never resorts results. Searching file contents is a later capability, not a prerequisite for filename integration.

## Required operation: preview

Request:

```json
{"version":1,"id":"preview-1","op":"preview","path":"C:\\notes\\report.md","query":"report","max_bytes":65536,"context_lines":3}
```

Text response (plain text, not Markdown):

```json
{"version":1,"id":"preview-1","ok":true,"result":{"kind":"text","text":"Quarterly report results\nRevenue increased.","start_line":42,"truncated":true}}
```

`start_line` is 1-based and applies to the first returned line. Return one contiguous excerpt. Prefer the first matching content line with surrounding context; if no content match exists, return the beginning of the file. The query can be a filename match, so absence of a content match is normal. Text output must be valid Unicode and bounded by max_bytes after UTF-8 encoding. Decode UTF-8 (and optionally UTF-16 BOM) without splitting characters. Skip binaries/unsupported types; don't send arbitrary binary data as text. Backend chooses the preview allowlist and enforces size/time limits, particularly for offline WSL/UNC files.

Image response (optional):

```json
{"version":1,"id":"preview-1","ok":true,"result":{"kind":"image","image_path":"C:\\Users\\you\\AppData\\Local\\GoodSearch\\previews\\thumbnail.png"}}
```

Supply a bounded local thumbnail (recommended <= 1024 px, < 5 MB), not an HTTP URL. Command Palette loads this image path itself. Keep generated thumbnails alive for the palette session. Backend should only return images the user authorized through configured roots or a backend-owned preview cache.

Unsupported preview is a successful response:

```json
{"version":1,"id":"preview-1","ok":true,"result":{"kind":"unavailable","message":"Preview is not supported for this file type."}}
```

An initial backend may return `unavailable` for every file while text previews are implemented.

## Errors

```json
{"version":1,"id":"preview-1","ok":false,"error":{"code":"not_indexed","message":"File is outside the configured search roots."}}
```

Recommended codes: `invalid_request`, `unsupported_version`, `unsupported_operation`, `not_indexed`, `not_found`, `unavailable`, `busy`, `internal_error`. `result` may be omitted on error; `error` may be omitted on success. Never return success without a result. Reject malformed/missing required fields rather than substituting defaults. The client validates response version and ID.

## Backend ownership

Expose the existing Rust search library through a headless, long-lived process or mode; keep watching/reconciliation outside the extension lifecycle. Reuse existing folder/exclusion config. Validate preview paths against currently authorized roots/index membership, resolve links safely, and enforce exclusions before reading; don't make the pipe an arbitrary file-read endpoint. Do not log query text or file contents by default.

No config-write, reindex, shutdown, or file-open IPC operation is required in v1. The extension opens the returned file using Windows shell associations, reveals it in Explorer, and copies its path locally.
