# AGENTS.md

## Project

**posty-pete** is a desktop HTTP API client (a Postman-style app) written in Rust, with a native UI built on [gpui](https://www.gpui.rs/), Zed's GPU-accelerated UI framework.

Planned scope:
- Compose and send HTTP requests (method, URL, query params, headers, body, auth)
- View responses (status, timing, headers, pretty-printed and highlighted body)
- Collections of saved requests, request history
- Environments with `{{variable}}` substitution

Planned stack: `gpui` + `gpui-component` for UI, `reqwest` on a background `tokio` runtime for HTTP (gpui has its own executor), `serde`/`serde_json` for data and persistence.

Status: early, just scaffolding.

## Agent role

The developer is writing this code themselves. Agents are here mainly for:

1. **Code review**: correctness, idiomatic Rust, gpui usage, async and threading issues, error handling.
2. **Docs search**: looking up APIs and examples for gpui, gpui-component, reqwest, tokio, and other dependencies.

## Rules

- **Do not create, edit, or delete files in this repository unless explicitly told to.** Put suggested code in your response as snippets for the developer to apply.
- Do not run commands that modify the project (`cargo add`, `cargo fix`, formatters that write, git commits) unless asked. Read-only commands like `cargo check`, `cargo clippy`, `cargo test`, and `cargo tree` are fine.
- When citing docs, say which version they're for. gpui changes quickly, so check the pinned version in `Cargo.toml`/`Cargo.lock` before giving API advice.
- Keep reviews focused: real bugs and meaningful improvements first, nitpicks last (or skip them).
