# gpui for React developers

A mapping of React concepts to gpui, written against **gpui-pre 0.3.11** (the gpui build that `gpui-component` 0.9 depends on). gpui changes quickly, so double-check APIs against the pinned version.

Many of the ideas carry over. The biggest change is **where state lives and how you change it**, because Rust's ownership rules shape the whole API.

## Concept map

| React | gpui |
|---|---|
| Component with state (`useState`) | A struct that implements `Render`. State goes in **struct fields**; there are no hooks. |
| Stateless component that takes props | A struct with `#[derive(IntoElement)]` that implements `RenderOnce`. It's built fresh on every render, and its fields are the props. |
| `setState(...)` | Change the fields, then call **`cx.notify()`**. Nothing re-renders until you call it. |
| JSX | Builder chains: `div().flex().child(...)` |
| CSS / Tailwind | Tailwind-style methods (`.p_4()`, `.gap_2()`, `.bg(...)`) and a flexbox and grid layout engine (taffy) |
| `onClick={() => ...}` | `.on_click(cx.listener(\|this, event, window, cx\| { ... }))`. The closure gets `&mut Self` back, so you don't capture state in it. |
| `key` prop | `.id("some-id")`. Elements need an ID for anything stateful: click, hover, scroll or drag. |
| Lifting state up / passing callbacks | **`EventEmitter`**. The child calls `cx.emit(event)` and the parent calls `cx.subscribe(&child, ...)`. |
| `useEffect` on someone else's state | `cx.observe(&entity, ...)` re-runs when that entity calls `notify()`. |
| Context / Redux store | **Globals**: `cx.set_global(...)` and `cx.global::<T>()`. Or a shared `Entity<Store>` that views observe. |
| `useRef` to a child component | Keep an `Entity<Child>` in a field, and call `child.update(cx, \|c, cx\| ...)` to change it. |
| `fetch` with `useEffect` cleanup | `cx.spawn(async move \|this, cx\| ...)` returns a `Task`. **Dropping the Task cancels it**, so store it in a field or call `.detach()`. |
| Keyboard shortcuts | **Actions** together with key bindings, plus `FocusHandle` for focus. This is more structured than React and closer to a desktop app. |

## What actually feels different

### 1. The app owns all the state; you get handles to it

An `Entity<T>` is a reference-counted handle. The `T` behind it lives in the `App`, and you reach it through the context:

```rust
let count = self.counter.read(cx).count;                          // read
self.counter.update(cx, |c, cx| { c.count += 1; cx.notify(); });  // write
```

You can't hold a `&mut` to another view across calls; every access goes through `cx`. This feels clunky for the first week, but it's how gpui avoids `Rc<RefCell>` everywhere.

### 2. Re-renders are explicit and there's no diffing

React re-renders when state changes and then diffs a virtual DOM. gpui calls `render()` only for views that called `cx.notify()` (or whose parent re-rendered), and it rebuilds that view's element tree from scratch. It doesn't diff or reconcile. That's cheap because elements are lightweight.

If the UI doesn't update, you almost always forgot to call `cx.notify()`.

### 3. Event handlers don't capture state

In React, a closure captures `count` and `setCount`. In gpui, `cx.listener` gives you `this: &mut Self` when the handler runs:

```rust
div()
    .id("send")
    .child("Send")
    .on_click(cx.listener(|this, _event, _window, cx| {
        this.sending = true;
        cx.notify();
    }))
```

### 4. Choosing between stateful and stateless pieces

- If a part of the UI has its own state that persists, such as the request editor, response viewer or sidebar, make it an `Entity` that implements `Render`.
- If it's just markup with props, such as a method badge, header row or status pill, make it `RenderOnce`. You can also use a plain function that returns `impl IntoElement`, which is the equivalent of a small function component.

### 5. Cyclic references leak memory

If a child holds an `Entity` of its parent and the parent holds the child, neither is ever dropped. Use `WeakEntity<T>` for back-references; `cx.spawn` already gives you a weak `this` for this reason.

## How this applies to Posty Pete

A natural structure:

- `Workspace` (root entity) owns `Entity<Sidebar>`, `Entity<RequestEditor>` and `Entity<ResponseView>`.
- When you press Send, `RequestEditor` emits `SendRequested(Request)`.
- `Workspace` subscribes to that event. It starts the HTTP call on the tokio thread, `cx.spawn`s a task that awaits the result, and then updates `ResponseView`.
- Collections and environments go in a `Global` or a shared `Entity<Store>`.

That's the same "state up, events up, data down" pattern you'd use in React. The differences are that it's spelled out with entities and subscriptions instead of props and callbacks, and that every re-render is explicit.

## Further reading

Examples in `~/.cargo/registry/src/index.crates.io-*/gpui-pre-0.3.11/examples/`:

- `input.rs`: focus, actions and an `Entity` with events
- `ownership_post.rs`: entities, `update` and `observe`
- `uniform_list.rs`, `data_table.rs`: virtualized lists and tables (useful for history and headers)
