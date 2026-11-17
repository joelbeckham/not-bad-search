# Gmail sources

In Settings → Sources, choose **Add Gmail**. Sources start paused. Set a Gmail search query (such as `in:inbox`, `label:engineering`, or `from:person@example.com`) and history start date, unpause, then save. The default is Inbox with a start date 30 days ago. The source uses the Gmail account connected to the shared Bifrost connection in Processing.

Each search result represents a complete conversation. Not Bad Search indexes subjects, senders, recipients, message dates and plain text, including earlier messages in conversations that match the query/date boundary. Previews remain local; **Open in Gmail** opens the connector-provided account-specific link. Date sorting uses the newest message date in the conversation.

Sync uses only read-only search and conversation tools and does not mark messages read. Pages resume after failures; each page is published atomically. Every cycle rechecks the matching scope because this connector has no history/change-feed endpoint. Unchanged text retains its chunks and embeddings. Narrow the query or increase the polling interval for large mailboxes. Changing the query or date restarts the scan; prior local copies remain until the source is removed, including mail deleted remotely or no longer matching the query.

Attachment contents and embedded images are not indexed: the deployed Bifrost Gmail tool catalog does not expose an attachment download endpoint. Rendering email text does not load remote tracking images.

Developer checks: `cargo test --test gmail`; `bifrost-check gmail-smoke` reads at most one matching conversation and indexes it into an in-memory database without changing the application's index.
