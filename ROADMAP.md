# Roadmap

Functionality that's planned but not built yet. Add items here when they come up and don't fit the current work; remove them when they're done.

## Requests

- [ ] **URL ↔ Params two-way sync.** Typing `?page=2` in the URL fills the Params table, and editing the table rewrites the URL (Postman-style). Parse with the `url` crate and guard against update loops. Today the table is the source of truth and reqwest appends params at send time, so a query string in the URL *and* table params are both sent.
- [ ] **Auth: API key in query params.** An "Add to: Header / Query params" option, like Postman's.
- [ ] **Auth: more types.** OAuth 2.0, Digest, AWS Signature.
- [ ] **Secrets at rest.** Once requests are persisted, don't store passwords and tokens in plain JSON. Use the OS keychain (`keyring` crate) or environment variables.
- [ ] **Body: more modes.** `x-www-form-urlencoded`, file fields in form-data, more raw kinds (XML, HTML).
- [ ] **Body: JSON helpers.** Validate and prettify raw JSON.
- [ ] **Body: Cmd+Enter to send** from the raw editor (`InputEvent::PressEnter { secondary: true, .. }`).
- [ ] **Param descriptions.** Description column in the Params table (removed for now).
- [ ] **Friendlier TLS errors.** When the error chain contains rustls `InvalidContentType` (server answered in plain HTTP), show a hint: "The server didn't respond with TLS. Did you mean http://?"
- [ ] **Loading state.** Show progress on the Send button while `BaseLayout::pending` is `Some`.
- [ ] **Cancel in-flight requests.** Replacing `pending` drops the gpui task, but the request keeps running on tokio.

## Responses

- [ ] **Content sniffing fallback.** When `Content-Type` is wrong or missing, detect JSON from the body (starts with `{`/`[` and parses).
- [ ] **XML highlighting.** gpui-component 0.9 has no XML grammar; XML is highlighted as HTML for now.
- [ ] **Large responses.** The editor handles ~50K lines; add a size cutoff or a "response too large to display" message.

## App

- [ ] **Global keybindings via actions.** `cmd-enter` send, `cmd-,` settings, `cmd-b` toggle sidebar. Buttons and menus dispatch the same actions (`actions.rs`).
- [ ] **Settings screen.** `StatusBarEvent::OpenSettings` only prints for now.
- [ ] **Collections, history, environments.** Sidebar content, persistence, and `{{variable}}` substitution (see Planned scope in AGENTS.md). The sidebar becomes a view once it holds state.
- [ ] **Extract `Workspace` into `app.rs`.** `BaseLayout` currently lives in `main.rs`.
- [ ] **Persist layout.** Remember sidebar width / request-response split across restarts.

## Look and feel

- [ ] **Custom theme.** Own theme JSON with a colored `primary` (instead of hardcoding colors), loaded via `ThemeRegistry`.
- [ ] **Extra icons.** Own `AssetSource` for icons outside the default 106 (e.g. `Send`, `Trash`, `Clock`), falling back to `gpui_kit_assets::Assets`.

## Quality

- [ ] **Unit tests for `BodyFormat::from_content_type`.** `application/json; charset=utf-8`, `application/problem+json`, `application/xml`, `text/html`, `application/x-yaml`, missing and invalid headers.
