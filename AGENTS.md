# AGENTS.md

## Project

**posty-pete** is a desktop HTTP API client (a Postman-style app) written in Rust, with a native UI built on [gpui](https://www.gpui.rs/), Zed's GPU-accelerated UI framework.

Planned scope:
- Compose and send HTTP requests (method, URL, query params, headers, body, auth)
- View responses (status, timing, headers, pretty-printed and highlighted body)
- Collections of saved requests, request history
- Environments with `{{variable}}` substitution

Status: early, just scaffolding.

## Stack

- **UI:** `gpui-component` 0.9 on top of gpui.
  - The gpui crate is **`gpui-pre` =0.3.11, renamed to `gpui`** in `Cargo.toml`, because that's the gpui build `gpui-component` 0.9 depends on. Don't add the crates.io `gpui` (0.2.x) crate; it would pull in a second, incompatible gpui. Look up gpui APIs in `gpui-pre-0.3.11`, not in gpui 0.2 docs.
  - The platform layer is **`gpui-pre-platform` =0.3.11, renamed to `gpui_platform`**, and the app starts with `gpui_platform::application()`. It **must** have the `font-kit` feature enabled, or macOS uses a no-op text system and renders no text.
  - The app calls `gpui_component::init(cx)` at startup, and the root view of each window is wrapped in `gpui_component::Root`.
- **HTTP:** `reqwest` 0.13 (rustls backend) on a background `tokio` runtime. gpui has its own executor, so results get passed back to the UI via channels and `cx.spawn`. In reqwest 0.13, `query` and `form` are opt-in features.
- **Data:** `serde` / `serde_json` for models and persistence.
- **Logging:** `tracing` + `tracing-subscriber`. gpui reports many problems only through `log` warnings, so keep the subscriber initialized in `main`.
- **macOS build requirements:** full Xcode (not just the Command Line Tools), first-launch setup done (`xcodebuild -runFirstLaunch`), and the Metal Toolchain component (`xcodebuild -downloadComponent MetalToolchain`). gpui compiles Metal shaders at build time. The current gpui snapshot also needs a recent stable Rust (1.97 was too old).

## Code organization

Code is organized **by feature**, with all non-UI logic in a gpui-free `domain/` module.

```
src/
├── main.rs        # app startup: init, theme, open window
├── app.rs         # Workspace: root view, owns the panes, wires up events
├── actions.rs     # global actions + key bindings
├── request_editor # RequestEditor: editing a request (.rs now, folder when it grows)
├── response_view  # ResponseView: viewing a response
├── sidebar.rs     # Sidebar: collections, history
├── status_bar.rs  # StatusBar
├── ui/            # small shared RenderOnce pieces (badges, pills, etc.)
└── domain/        # pure Rust: models, HTTP, persistence, env variables
```

Rules:

- **`domain/` must not import gpui or gpui-component.** It holds request/response models, the HTTP client (reqwest on tokio), collections and persistence, and `{{variable}}` substitution. It must be testable with plain `cargo test`. It may later become a separate workspace crate. (It is named `domain` rather than `core` to avoid clashing with Rust's built-in `core` crate.)
- **One feature per module, named after its main type.** The module is the snake_case form of the view or component it contains (`RequestEditor` → `request_editor`, `StatusBar` → `status_bar`), and fields holding it use the same name (`request_editor: Entity<RequestEditor>`). A view's struct, `impl Render`, events (`EventEmitter`) and actions live together in its module. Start as a single file (`request_editor.rs`) and split it into a folder (`request_editor/mod.rs` + sub-files) only when it grows.
- **Views own UI state; models live in `domain/`.** For example, `RequestEditor` holds `Entity<InputState>`s and builds a `domain::Request` from them. Domain types never depend on widget types.
- **Children emit events and parents wire them up.** Sibling views don't reach into each other; `Workspace` (`app.rs`) subscribes to child events and coordinates.
- **Use `Entity` + `Render` for components with lasting state, and `RenderOnce` or plain functions for presentation.** Shared presentational pieces go in `ui/`; ones used by a single feature stay in that feature's folder.

When reviewing, flag violations of these rules, especially gpui imports in `domain/`.

## Agent role

The developer is writing this code themselves. Agents are here mainly for:

1. **Code review**: correctness, idiomatic Rust, gpui usage, async and threading issues, error handling.
2. **Docs search**: looking up APIs and examples for gpui, gpui-component, reqwest, tokio, and other dependencies.

## Reference material

The repo vendors two skills from `longbridge/gpui-kit` in `.agents/skills/` (pinned in `skills-lock.json`, symlinked into `.claude/skills/` for Claude Code):

- **`gpui-kit`**: Coding Guides, component conventions and usage (Input, Select, Button, Dialog...), an app recipe, and gpui references (entities, elements, layout/style, focus, actions, async, events, testing) in `references/gpui/`.
- **`gpui-kit-design-guides`**: design rules for layout, spacing, color, interaction states, overlays and interface copy, plus review checklists.

Consult them for gpui and gpui-component questions, but keep in mind:

- They're written for the umbrella **`gpui-kit`** crate (`use gpui_kit::component::...`, `gpui_kit::application()`, `gpui_kit::open_window`). This project depends on **`gpui-component`** and **`gpui-pre`** directly, so translate the paths: `gpui_kit::component::X` → `gpui_component::X`, GPUI items → `gpui::X`, `gpui_kit::application()` → `gpui_platform::application()`. Don't add `gpui-kit` as a dependency unless the developer decides to switch.
- They may lag behind the pinned versions (their setup snippet mentions `gpui-kit = "0.7"`). When a skill and the source disagree, the source in `~/.cargo/registry/src/*/gpui-component-0.9.0` (and `gpui-base-0.9.0`, `gpui-pre-0.3.11`) wins.

## Rules

- **Do not create, edit, or delete files in this repository unless explicitly told to.** Put suggested code in your response as snippets for the developer to apply.
- Do not run commands that modify the project (`cargo add`, `cargo fix`, formatters that write, git commits) unless asked. Read-only commands like `cargo check`, `cargo clippy`, `cargo test`, and `cargo tree` are fine.
- When citing docs, say which version they're for. gpui changes quickly, so check the pinned version in `Cargo.toml`/`Cargo.lock` before giving API advice.
- Keep reviews focused: real bugs and meaningful improvements first, nitpicks last (or skip them).
