# Jira through Bifrost

Good Search uses a deterministic MCP client for background imports. Searches, previews, inspection, embeddings and ranking use the Windows-local index; typing never calls Jira or Bifrost.

## Setup

1. In Settings → Processing, enter the Bifrost MCP endpoint and API key, then **Store connection**. Both are stored in Windows Credential Manager under `GoodSearch/Bifrost/default`; neither is written to config.toml. **Test saved connection** discovers tools and checks the ordinary Jira account.
2. In Sources, choose **Add Jira**. Defaults are site `https://bombbomb.atlassian.net`, cloud ID `bombbomb.atlassian.net`, and project `CORE` (project ID 10006). Start with a JQL expression such as `key in (CORE-123, CORE-456)` using real issue keys. Page size is a pagination setting, not an import limit.
3. New sources start paused. Set the JQL, uncheck pause and save. Use **Sync enabled Jira sources now** for manual refresh; this operates on saved settings. Default background polling is five minutes.

The source's project condition is always combined with the JQL. User ordering is replaced with updated ascending. Summaries, descriptions and supported image attachments are imported. Comments and non-image attachment contents are excluded.

## Protocol and security

The Streamable HTTP MCP client initializes a session and paginates tools/list. It maps discovered names to a fixed allowlist: `jira_atlassianUserInfo`, `jira_getAccessibleAtlassianResources`, `jira_getVisibleJiraProjects`, `jira_searchJiraIssuesUsingJql`, `jira_getJiraIssue`. Exact names, the verified `jira-` prefix, or MCP double-underscore namespace prefixes are accepted. Service-user tools and writes are rejected. Ambiguous ordinary Jira tool mappings fail visibly. Use the server's `/mcp` endpoint, not its website root.

Requests use Bearer authentication. HTTPS is required except loopback HTTP for synthetic tests; redirects are disabled. Requests have a 45-second timeout and 4 MiB response limit, support JSON and SSE response envelopes, and do not log the endpoint, key, or raw error bodies. HTTP 401/403 are reported as authentication/access failures. Tool-level errors remain generic because the gateway's error taxonomy has not been verified.

## Sync and storage

Schema 4 adds remote_items, remote_sync and remote_failures. A remote item has a stable internal URI backed by a files row so existing FTS, chunks and vectors can be reused. Remote URIs are excluded from filesystem scanning and extraction. Visible locations and opening use the original HTTPS Jira URL.

Jira v2 search and detail requests prefer Markdown and request the full view. When Jira returns HTML, appliedContentFormat selects a standard HTML parser that preserves readable structure and link destinations. Raw content and its actual format are retained. Markdown is indexed directly. Legacy ADF remains readable during migration. Descriptions exceeding 2 MiB fail without publishing partial text.

Network and conversion occur outside SQLite write transactions. Each complete page publishes its items and continuation token atomically. Duplicate content preserves chunk/vector identities; changed content atomically replaces chunks, cascading old vectors, then queues local embedding regeneration. A run checkpoint advances only after the last page succeeds. Timestamps normalize to UTC; incremental JQL uses a date-only lower bound at least 48 hours before the checkpoint to overlap Jira timezone boundaries. Resuming uses the saved fixed JQL and page token. After repeated failures, a run restarts from its last completed checkpoint to recover from expired tokens. Retries back off from 30 seconds to one hour. Work yields after 50 pages per worker pass.

Pausing/removing a source or changing its saved scope cancels publication at request/page boundaries. An in-flight request can take up to its timeout. Source removal explicitly removes its local records. Missing search results **never** delete indexed issues. Scope changes retain prior items and flag those not confirmed in the new scope. Source status exposes last successful sync, pending progress, errors, unresolved issues, content warnings and embedding counts. Inspect indexed data exposes raw content and its format, per-item sync timestamps and failures.

## Limits and validation

Deletion feeds, access-change feeds, complete comment pagination and tool error classification remain unverified. Incremental JQL cannot prove that an absent issue still exists or remains accessible; cached results may be stale. The local source timestamp is freshness information, not a deletion guarantee.

`cargo test --offline --locked --tests` covers page recovery, checkpoint integrity, duplicates, edit-driven chunk/vector replacement, a large description, ADF boundaries/links/unsupported nodes, cancellation, transient errors, remote URL resolution, JSON/SSE parsing and read-only discovery over a synthetic HTTP server. `scripts/jira-ui-smoke.ps1` runs an isolated, paused synthetic source through the native UI. `bifrost-check fixture <new-directory>` prepares that fixture without network access. `bifrost-check catalog` discovers live allowed tools, and `bifrost-check seed <new-directory>` selects two recent CORE keys, pins them in JQL, imports one issue per page and saves a paused source. The seed command does not modify the normal app configuration.

### Windows validation, November 6, 2026

Live MCP initialization and discovery succeeded at `/mcp`, using Streamable HTTP. The server root returned HTTP 405; it was not an SSE transport issue. Actual names are `jira-atlassianUserInfo`, `jira-getAccessibleAtlassianResources`, `jira-getVisibleJiraProjects`, `jira-searchJiraIssuesUsingJql`, and `jira-getJiraIssue`. Account verification returned Joel Beckham.

Selected two recently updated CORE issues and pinned `key in (CORE-10825, CORE-11121)`. Search returned structured ADF directly (280 and 8,393 serialized bytes). Import completed with one issue per page. Repeated overlapping incremental runs preserved chunk IDs. Three chunks were embedded locally; a content-derived query returned two semantic matches with Jira URLs. Text search over this tiny fixture measured p50 0.395–0.463 ms and p95 0.513–1.116 ms across two 100-query runs; this is not a large-corpus benchmark.

Native release checks passed for search, Enter preview, Up/Down preview navigation, Escape, right-click inspector, Sources/Processing settings and Enter opening the Jira URL through Windows ShellExecute. Captures and worker traces are in ignored `test-data` fixtures. The isolated app measured 105–117 MiB working set and 1.25–3.13% of one CPU core over brief five-second visible-window samples (including the blinking caret); these are not long-duration idle measurements. Its hotkey conflict banner was expected because the normal app remained running.

Synthetic tests cover large descriptions and ADF tables/media warnings. Live ADF table/media behavior, deletion/access-change feeds and complete error classification remain unverified.

Installed the release after backing up the normal data directory and executable. Existing folder settings were preserved and the pinned two-key Jira source enabled. The normal app imported both issues and generated all three vectors without warnings or unresolved failures. Its global hotkey showed/focused the window in 30–48 ms over three trials; empty-search Escape hid it. A ten-second hidden sample measured 0.469% of one CPU core and 134.8 MiB working set with the existing index/model loaded. All 36 automated tests and Clippy with warnings denied passed. Changes remain uncommitted.

## Image attachments

The ordinary `jira_v2-downloadJiraIssueAttachment` tool supplies short-lived URLs; gateway credentials are never forwarded to download hosts. The v2 ordinary-account tools take precedence for search and issue detail.

Supported attachments are PNG, JPEG and WebP, up to 10 MiB and 20 megapixels. Downloads are bounded, cancellable between requests, and occur outside SQLite transactions. A source error triggers existing sync retry/backoff. Missing attachments are retained conservatively. Manual sync rescans parent attachments; unchanged hashes preserve chunks and vectors. Changed bytes replace derived content.

Images are cached under the data directory's `attachments` folder, analyzed through the configured OpenAI model and existing daily budget, then indexed locally. Source pause and global image-processing pause are respected. Each image is its own Image result with a thumbnail; preview uses cached bytes and Enter opens the parent Jira ticket. The inspector includes attachment metadata and the parent relationship. Search and preview require no network calls.

## Reloading existing tickets

The schema upgrade schedules one full refresh for each source. Paused sources wait until resumed. Use Reload all tickets on next save, then Save changes, to repeat a full refresh of the saved JQL scope. It retains the last usable local snapshot, replaces refreshed content atomically, and does not delete issues missing from the query. The normal Sync enabled Jira sources now button remains incremental.

Mixed-format search pages are resolved per issue. Ambiguous descriptions use a detail request (Markdown preferred, HTML if that detail is still mixed). The full v2 response omits empty descriptions; those are normalized to empty content. Page checkpoints advance only after all descriptions are resolved.

Results show the exact status with a category icon and a relative Jira updated date (hover for the full local timestamp). Previews include status, updated time, assignee, issue type, priority, and project, with created time, reporter, labels, and sync time under Ticket details. Attachment metadata is labeled as belonging to the parent ticket. Missing optional fields are omitted or shown as unavailable. Schema v7 stores metadata separately from searchable content; upgrading triggers a scope refresh and metadata-only changes preserve chunks and embeddings.
