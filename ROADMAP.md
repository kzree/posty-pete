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

- [ ] **Environments in quick open.** List shared and collection environments in quick open to jump to their editor.
- [ ] **Library keyboard navigation skips empty rows.** Up/Down stops on the "No collections yet" / "No environments yet" rows; gpui-component 0.9's List has no disabled-row skipping, so it needs our own SelectUp/SelectDown handling.
- [ ] **Revisit library empty rows.** The muted "No collections yet" / "No environments yet" rows replaced "Your library is empty." Revisit the empty state later, e.g. make the rows actionable ("Create a collection") or bring back a first-run message.
- [ ] **One save queue for stores.** `SharedEnvironments` copies the ordered save queue from `Collections`; extract a shared helper if a third store appears.
- [ ] **Environment colours.** Mark an environment (e.g. red for Prod) in the picker and title bar so sending to production is hard to miss.
- [ ] **Duplicate environments.** Copy an environment with its variables, e.g. Staging → Prod.
- [ ] **Truncate long names in the environment menu.** `PopupMenuItem` wraps long labels; needs a custom element item or an upstream change.
- [ ] **Variable autocomplete and highlighting.** Suggest names after typing `{{` and highlight references (resolved vs unresolved) in the URL, key/value and body inputs. `domain::variables::references` returns byte ranges for this.
- [ ] **Folder-level variables.** Variables on folders, overriding the collection's for requests inside them.
- [ ] **Manage requests in collections.** Duplicate and move requests (and move folders) in the library tree, including drag and drop.
- [ ] **Unsaved changes on Dock Quit / logout.** cmd+Q, the Quit menu item and closing the window ask before discarding edits, but Dock "Quit", logout and shutdown bypass the app's Quit action (gpui-macos 0.3.11 doesn't implement `applicationShouldTerminate:`), so edits are dropped without asking. Relatedly, a collection-variables edit made in the last half second before a Dock quit may not be written: gpui waits only 200 ms for quit handlers.
- [ ] **Unsaved collection name edits.** `CollectionView::show` keeps a typed but unsaved name when you leave a collection and come back to it. Decide whether it should be discarded instead.
- [ ] **Single source for the editor's saved-request link.** `Workspace.open_request` and `RequestEditor.saved_name` both hold the link, kept in sync only because every caller runs `load_request` then `set_open_request`. Pass the saved name into `load_request` (or let `Workspace` unlink explicitly) so they can't drift apart.

## App

- [ ] **In-window menu bar on Windows/Linux.** The native menu bar (`src/menus.rs`) only shows on macOS.
- [ ] **Apply and save settings.** The Settings page (General, Appearance, Requests, History & data, AI, Shortcuts, About) is UI only: controls keep local state and change nothing. Persist them (e.g. `settings.json`) and apply them; wiring notes are in task 61.
- [ ] **Customizable shortcuts.** Let users rebind the actions listed on the Shortcuts settings page.
- [ ] **About links.** Make the licence/project URLs on the About page open in the browser, and add repository/licence links once `Cargo.toml` declares them.
- [ ] **History: store responses.** History saves only request + status/timing for now; optionally keep (capped) response bodies.
- [ ] **History retention setting.** 14 days is hard-coded (`RETENTION_DAYS` in `domain/history.rs`); make it configurable on the Settings page.
- [ ] **History: prune while running.** Pruning only happens at startup (`History::new`), so an app left open for days keeps entries older than 14 days in the panel and the database, and "Today"/"Yesterday" labels don't roll over at midnight. Prune on a daily timer or on each record.
- [ ] **History: limit stored request body size.** Every sent request is saved with its full body, so large payloads sent repeatedly grow `history.db` quickly. Skip or truncate bodies over a cap (e.g. 1 MB), or dedupe identical bodies by hash.
- [ ] **Navigation: two-finger swipes.** Mouse side buttons and three-finger "swipe between pages" work; two-finger trackpad swipes arrive as plain scrolling and would need custom horizontal-scroll detection.
- [ ] **Persist layout.** Remember sidebar width / request-response split across restarts.

## Look and feel

- [ ] **Custom theme.** Own theme JSON with a colored `primary` (instead of hardcoding colors), loaded via `ThemeRegistry`.

## Quality

- [ ] **Unit tests for `BodyFormat::from_content_type`.** `application/json; charset=utf-8`, `application/problem+json`, `application/xml`, `text/html`, `application/x-yaml`, missing and invalid headers.
