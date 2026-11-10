# Roadmap

Functionality that's planned but not built yet. Add items here when they come up and don't fit the current work; remove them when they're done.

## Requests

- [ ] **URL ↔ Params two-way sync.** Typing `?page=2` in the URL fills the Params table, and editing the table rewrites the URL (Postman-style). Parse with the `url` crate and guard against update loops. Today the table is the source of truth and reqwest appends params at send time, so a query string in the URL *and* table params are both sent.
- [ ] **Auth: API key in query params.** An "Add to: Header / Query params" option, like Postman's.
- [ ] **Auth: more types.** OAuth 2.0, Digest, AWS Signature.
- [ ] **Secrets at rest.** Once requests are persisted, don't store passwords and tokens in plain JSON. Use the OS keychain (`keyring` crate) or environment variables.
- [ ] **Body: more modes.** `x-www-form-urlencoded`, file fields in form-data, more raw kinds (XML, HTML).
- [ ] **Body: JSON helpers.** Validate and prettify raw JSON.
- [ ] **Param descriptions.** Description column in the Params table (removed for now).
- [ ] **Friendlier TLS errors.** When the error chain contains rustls `InvalidContentType` (server answered in plain HTTP), show a hint: "The server didn't respond with TLS. Did you mean http://?"
- [ ] **Structured send errors.** Send failures travel as `Result<Response, String>` (`flatten_send_result` and `finish_request` in `app.rs`, `ResponseView::set_response`), so the error chain is flattened to text before the UI sees it. Keep an `anyhow::Error` (or a small `SendError { message, hint }` in `domain/`) so hints like the TLS one above can inspect the chain. Do it together with that item.
- [ ] **Request timeout setting.** The 30s timeout (`REQUEST_TIMEOUT` in `app.rs`) is hard-coded; make it configurable on the Settings page and per request.
- [ ] **Cancel in-flight requests.** Send is disabled while a request runs; a Cancel button could take its place. Dropping `pending` alone doesn't stop the request on tokio (abort the `JoinHandle`).

## Responses

- [ ] **Content sniffing fallback.** When `Content-Type` is wrong or missing, detect JSON from the body (starts with `{`/`[` and parses).
- [ ] **XML highlighting.** gpui-component 0.9 has no XML grammar; XML is highlighted as HTML for now.
- [ ] **Large responses.** Downloads are capped at 50 MB (`MAX_BODY_BYTES` in `domain/http.rs`; larger bodies fail with "response exceeds 50 MB"), but the editor handles only ~50K lines; add a display cutoff or a "response too large to display" message, and possibly make the download cap configurable.

## Collections

- [ ] **Collection variables.** The collection view's Variables tab is a placeholder. Decide the requirements (scope, secret values, how they combine with environments) and build the editor, likely on top of `ui::key_value_editor`.
- [ ] **Manage requests in collections.** Duplicate and move requests (and move folders) in the library tree, including drag and drop.
- [ ] **Unsaved changes on quit.** Edited saved requests are marked and prompt before being replaced, but closing the window or quitting doesn't ask yet.
- [ ] **Revert a saved request.** Clicking the request already in the editor keeps its edits, so there's no way to throw them away except navigating elsewhere and choosing Discard. Add a Revert action (e.g. in the request context menu or next to Save).
- [ ] **Save as.** Save a request that isn't linked to a collection (a fresh one or one from history) into a chosen collection or folder.
- [ ] **Unsaved collection name edits.** `CollectionView::show` keeps a typed but unsaved name when you leave a collection and come back to it. Decide whether it should be discarded instead.
- [ ] **Cheaper library refresh.** Every `CollectionsEvent::Changed` clones all collections into `LibraryPanel::set_collections`. Fine at current sizes; revisit if collections get large.
- [ ] **Single source for the editor's saved-request link.** `Workspace.open_request` and `RequestEditor.saved_name` both hold the link, kept in sync only because every caller runs `load_request` then `set_open_request`. Pass the saved name into `load_request` (or let `Workspace` unlink explicitly) so they can't drift apart.

## App

- [ ] **Native menu bar.** `cx.set_menus` listing the actions in `actions.rs` with their shortcuts.
- [ ] **Settings page.** Settings moved out of the sidebar; the status bar button and cmd+, only log for now (`OpenSettings` in `app.rs`). Needs a router to show it as its own page (theme, request timeout, history retention, ...).
- [ ] **Library, history, environments.** Content for the sidebar's Library and History tabs, persistence, and `{{variable}}` substitution (see Planned scope in AGENTS.md).
- [ ] **History: store responses.** History saves only request + status/timing for now; optionally keep (capped) response bodies.
- [ ] **History retention setting.** 14 days is hard-coded (`RETENTION_DAYS` in `domain/history.rs`); make it configurable on the Settings page.
- [ ] **History: prune while running.** Pruning only happens at startup (`History::new`), so an app left open for days keeps entries older than 14 days in the panel and the database, and "Today"/"Yesterday" labels don't roll over at midnight. Prune on a daily timer or on each record.
- [ ] **History: limit stored request body size.** Every sent request is saved with its full body, so large payloads sent repeatedly grow `history.db` quickly. Skip or truncate bodies over a cap (e.g. 1 MB), or dedupe identical bodies by hash.
- [ ] **Navigation: two-finger swipes.** Mouse side buttons and three-finger "swipe between pages" work; two-finger trackpad swipes arrive as plain scrolling and would need custom horizontal-scroll detection.
- [ ] **Navigation: persist history.** Keep the back/forward history across restarts.
- [ ] **Navigation: keep responses.** Going back or forward clears the response pane; keep each history entry's last response (capped) and show it again.
- [ ] **Persist layout.** Remember sidebar width / request-response split across restarts.

## Look and feel

- [ ] **Custom theme.** Own theme JSON with a colored `primary` (instead of hardcoding colors), loaded via `ThemeRegistry`.

## Quality

- [ ] **Unit tests for `BodyFormat::from_content_type`.** `application/json; charset=utf-8`, `application/problem+json`, `application/xml`, `text/html`, `application/x-yaml`, missing and invalid headers.
