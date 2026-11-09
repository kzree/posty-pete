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
- [ ] **Request timeout setting.** The 30s timeout (`REQUEST_TIMEOUT` in `main.rs`) is hard-coded; make it configurable in the Settings tab and per request.
- [ ] **Cancel in-flight requests.** Send is disabled while a request runs; a Cancel button could take its place. Dropping `pending` alone doesn't stop the request on tokio (abort the `JoinHandle`).

## Responses

- [ ] **Content sniffing fallback.** When `Content-Type` is wrong or missing, detect JSON from the body (starts with `{`/`[` and parses).
- [ ] **XML highlighting.** gpui-component 0.9 has no XML grammar; XML is highlighted as HTML for now.
- [ ] **Large responses.** Downloads are capped at 50 MB (`MAX_BODY_BYTES` in `domain/http.rs`; larger bodies fail with "response exceeds 50 MB"), but the editor handles only ~50K lines; add a display cutoff or a "response too large to display" message, and possibly make the download cap configurable.

## App

- [ ] **Native menu bar.** `cx.set_menus` listing the actions in `actions.rs` with their shortcuts.
- [ ] **Settings tab content.** The sidebar's Settings tab is a placeholder (theme, request timeout, ...).
- [ ] **Library, history, environments.** Content for the sidebar's Library and History tabs, persistence, and `{{variable}}` substitution (see Planned scope in AGENTS.md).
- [ ] **History: store responses.** History saves only request + status/timing for now; optionally keep (capped) response bodies.
- [ ] **History retention setting.** 14 days is hard-coded (`RETENTION` in `domain/history.rs`); make it configurable in the Settings tab.
- [ ] **Extract `Workspace` into `app.rs`.** `BaseLayout` currently lives in `main.rs`.
- [ ] **Persist layout.** Remember sidebar width / request-response split across restarts.

## Look and feel

- [ ] **Custom theme.** Own theme JSON with a colored `primary` (instead of hardcoding colors), loaded via `ThemeRegistry`.

## Quality

- [ ] **Unit tests for `BodyFormat::from_content_type`.** `application/json; charset=utf-8`, `application/problem+json`, `application/xml`, `text/html`, `application/x-yaml`, missing and invalid headers.
