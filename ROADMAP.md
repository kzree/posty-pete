# Roadmap

Functionality that's planned but not built yet. Add items here when they come up and don't fit the current work; remove them when they're done.

## Requests

- [ ] **URL ↔ Params two-way sync.** Typing `?page=2` in the URL fills the Params table, and editing the table rewrites the URL (Postman-style). Parse with the `url` crate and guard against update loops. Today the table is the source of truth and reqwest appends params at send time, so a query string in the URL *and* table params are both sent.
- [ ] **Auth: more types.** OAuth 2.0, Digest, AWS Signature.
- [ ] **Secrets at rest.** Once requests are persisted, don't store passwords and tokens in plain JSON. Use the OS keychain (`keyring` crate) or environment variables.
- [ ] **Body: file fields in form-data.** Form data is text-only; add file fields (multipart parts read from disk).
- [ ] **Param descriptions.** Description column in the Params table (removed for now).
- [ ] **Per-request timeout.** The global timeout is on Settings › Requests (task 82); let a request override it.

## Responses

- [ ] **XML highlighting.** gpui-component 0.9 has no XML grammar; XML is highlighted as HTML for now.
- [ ] **Large responses.** Downloads are capped at 50 MB (`MAX_BODY_BYTES` in `domain/http.rs`; larger bodies fail with "response exceeds 50 MB"), but the editor handles only ~50K lines; add a display cutoff or a "response too large to display" message, and possibly make the download cap configurable. The download cap doesn't bound the work after it: pretty-printing can grow the text well past 50 MB with no limit on formatted size or line count, and `ResponseView::set_response` loads it into the editor on the UI thread (again when switching to a cached response). Limit formatted bytes/lines while formatting, and skip prettifying/highlighting above a threshold.

## Collections

- [ ] **Revisit library empty rows.** The muted "No collections yet" / "No environments yet" rows replaced "Your library is empty." Revisit the empty state later, e.g. make the rows actionable ("Create a collection") or bring back a first-run message.
- [ ] **Environment colours.** Mark an environment (e.g. red for Prod) in the picker and title bar so sending to production is hard to miss.
- [ ] **Truncate long names in the environment menu.** `PopupMenuItem` wraps long labels; needs a custom element item or an upstream change.
- [ ] **Variable autocomplete and highlighting.** Suggest names after typing `{{` and highlight references (resolved vs unresolved) in the URL, key/value and body inputs. `domain::variables::references` returns byte ranges for this.
- [ ] **Folder-level variables.** Variables on folders, overriding the collection's for requests inside them.
- [ ] **Folder-level auth.** Auth on folders, used by requests set to Inherit inside them. Decide where it sits relative to the environment layers, matching folder variables.
- [ ] **Manage requests in collections.** Move requests (and move folders) in the library tree, including drag and drop.
- [ ] **Unsaved changes on Dock Quit / logout.** cmd+Q, the Quit menu item and closing the window ask before discarding edits, but Dock "Quit", logout and shutdown bypass the app's Quit action (gpui-macos 0.3.11 doesn't implement `applicationShouldTerminate:`), so edits are dropped without asking. Relatedly, a collection-variables edit made in the last half second before a Dock quit may not be written: gpui waits only 200 ms for quit handlers.
- [ ] **Unsaved collection name edits.** `CollectionView::show` keeps a typed but unsaved name when you leave a collection and come back to it. Decide whether it should be discarded instead.
- [ ] **OpenAPI: more of the spec.** Follow external `$ref`s (other files or URLs) and honour per-operation `servers` (every request uses `baseUrl` now).
- [ ] **OpenAPI: authenticated spec URLs.** Let the import/update fetch send headers or auth (bearer token, API key) for specs behind a login.
- [ ] **OpenAPI: drop a spec file on the window** to open the import dialog with it chosen.
- [ ] **OpenAPI: filter the file picker** to .json/.yaml/.yml once gpui's `PathPromptOptions` supports extension filters (0.3.11 has none).
- [ ] **OpenAPI: check for spec changes automatically.** On startup or on a timer, and by watching a local spec file.
- [ ] **OpenAPI: skip added operations in the update preview**, so an imported request you deleted doesn't come back on every update.
- [ ] **OpenAPI: update command.** A palette command and File menu item to update the current collection from its spec.
- [ ] **Export a collection as OpenAPI.**
- [ ] **Import Postman collections.**

## App

- [ ] **In-window menu bar on Windows/Linux.** The native menu bar (`src/menus.rs`) only shows on macOS.
- [ ] **Apply and save the remaining settings.** `settings.json` and the `AppSettings` global exist; theme and accent colour (tasks 65–67), the Requests page (task 82), the General page (task 83), the Appearance fonts and wrap (task 84) and History & data (task 85) are wired. The other Settings controls are still UI only; wire them one by one (notes in task 61).
- [ ] **MCP server.** A built-in MCP server so AI assistants (Claude Code, Claude Desktop, ...) can list collections and environments, send requests and read responses and history. Local only (127.0.0.1) with an access token, per-capability permissions and "ask before sending". Its settings UI is on Settings › AI (UI only so far). When wiring it: generate and keep a real token in the keychain, make Copy and Regenerate work, and decide whether the client snippet shows the real token.
- [ ] **Customizable shortcuts.** Let users rebind the actions listed on the Shortcuts settings page.
- [ ] **History: store responses.** History saves only request + status/timing for now; optionally keep (capped) response bodies.
- [ ] **Navigation: two-finger swipes.** Mouse side buttons and three-finger "swipe between pages" work; two-finger trackpad swipes arrive as plain scrolling and would need custom horizontal-scroll detection.
- [ ] **Palette recents: environments chosen inside the environment pages.** Selecting an environment on the shared environments page or a collection's Environments tab isn't recorded, because those views auto-select the first environment and would record ones you only glanced at. Record only deliberate selections.
- [ ] **Palette recents: back/forward and in-editor opens.** Items reached with back/forward, and requests opened while the editor is already showing, aren't recorded as recents. Decide whether they should be.
- [ ] **macOS system proxy.** Proxy "System" on Settings › Requests only reads the HTTP_PROXY/HTTPS_PROXY/NO_PROXY environment variables, which apps opened from Finder usually don't have. Reading the macOS network proxy settings needs reqwest's `system-proxy` feature.
- [ ] **Clamp settings numbers on blur.** The Settings number inputs have no number mask, so an out-of-range value stays visible in the field while the clamped value is saved (e.g. the field shows 0, 1 is saved). Clamp or reformat the field when it loses focus.

## Look and feel

- [ ] **Custom theme.** Own theme JSON with a colored `primary` (instead of hardcoding colors), loaded via `ThemeRegistry`.

## Quality

- [ ] **Upgrade gpui-component for the NumberInput focus fix.** Focusing a `NumberInput` (the Settings number fields, including via their −/+ buttons) panics in debug builds when an accessibility client is active: "set_focus called more than once in a single frame" (gpui-kit #3873, fixed by PR #3875 on main, not released as of gpui-component/gpui-base 0.9.1). Release builds only log a warning. Upgrade once a release includes it; this also moves gpui-pre past `=0.3.11`, so update `Cargo.toml` and AGENTS.md together.
- [ ] **Palette and library at scale.** Opening the palette rebuilds every entry, each keystroke sorts all matches, and a collection change refreshes every projection (library rows twice, hidden pages, environment clones). Benchmark with thousands of requests/environments/history entries before adding caches or narrower change events.
- [ ] **Presentation copies at scale.** The Headers tab clones every header name and value on each render, and the import and spec-update dialogs rebuild non-virtualized preview rows (copying warning and row strings). Build display strings once per response or plan, and virtualize long previews; benchmark with large responses and specs with thousands of operations first.
