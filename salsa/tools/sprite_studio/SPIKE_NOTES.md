# Sprite Studio GUI Spike Notes (Task 8)

De-risks the GPUI **0.2.2** APIs the real app will be built on. Every signature
below was confirmed by reading the actual gpui 0.2.2 source in the bazel external
dir (`crates__gpui-0.2.2/src/...`) and proven by `bazel build` of the
`//salsa/tools/sprite_studio:sprite_studio` binary (which links
`gui/src/lib.rs`). **The binary builds; it was not run** (running opens a
blocking window — that is a human verification step).

Tasks 9-13 should follow these signatures verbatim.

## Summary of deltas from the draft spec

| Item | Draft assumption | Reality in gpui 0.2.2 |
| --- | --- | --- |
| `PathPromptOptions` | 3 fields (`files`, `directories`, `multiple`) | **4 fields** — adds `prompt: Option<SharedString>` |
| `main.rs` imports | `use gpui::{... App ...}` was enough | `cx.new(...)` needs the **`AppContext` trait in scope** (`use gpui::AppContext;`); hello-world gets it via `prelude::*` |

Everything else in the draft (img source type, Timer, cx.spawn closure shape,
return type of the path prompt, cx.listener signature) matched the source as
written.

---

## (a) Redraw / advance timer

- `gpui::Timer` is a **re-export of `smol::Timer`** (`pub use smol::Timer;` in
  `src/gpui.rs`). `smol::Timer` is `async_io::Timer`.
- `Timer::after(Duration) -> Timer`, and `Timer` is itself a `Future` you
  `.await`. There is no callback form.

```rust
use gpui::Timer;
use std::time::Duration;

Timer::after(Duration::from_millis(500)).await;
```

Alternative (NOT used here, but available): the executor timer
`cx.background_executor().timer(duration) -> Task<()>`
(`Executor::timer` at `src/executor.rs:357`). The `gpui::Timer` form is simpler
and is what the spike uses.

Typical advance loop, driven by `cx.spawn` (see (e)):

```rust
cx.spawn(async move |this, cx| loop {
    Timer::after(Duration::from_millis(500)).await;
    if this.update(cx, |this, cx| { this.tick += 1; cx.notify(); }).is_err() {
        break; // entity dropped
    }
})
.detach();
```

## (b) Loading an image from a `PathBuf` and sizing it

- `gpui::img(source: impl Into<ImageSource>) -> Img` (`src/elements/img.rs:198`).
- `ImageSource` implements `From<PathBuf>`, `From<&Path>`, `From<Arc<Path>>`,
  `From<&str>`, `From<String>`, `From<SharedString>`, `From<SharedUri>`,
  `From<Arc<RenderImage>>`, `From<Arc<Image>>`
  (`src/elements/img.rs:56-118`). A local file path goes through the
  `From<PathBuf>` / `From<&Path>` impls — **no URI scheme or asset registration
  needed for a filesystem path**.
- `Img` implements `Styled` (`src/elements/img.rs:492`), so the standard
  sizing helpers `.w(px(..))`, `.h(px(..))`, `.size(..)` etc. are available
  directly on the result of `img(...)`.

```rust
use gpui::{img, px};
use std::path::PathBuf;

img(PathBuf::from("/abs/path/to/frame.png"))
    .w(px(200.))
    .h(px(200.))
```

## (c) Directory picker

- `App::prompt_for_paths(&self, options: PathPromptOptions)`
  (`src/app.rs:1116`). Reachable on a `Context<T>` because `Context` derefs to
  `App` (`impl Deref for Context { type Target = App; }`,
  `src/app/context.rs:25`). So `cx.prompt_for_paths(...)` works inside a
  `cx.listener` body.

- **Return type:** `oneshot::Receiver<Result<Option<Vec<PathBuf>>>>`
  (gpui's `Result` = `anyhow::Result`). Awaiting the receiver therefore yields
  `Result<Result<Option<Vec<PathBuf>>>, oneshot::Canceled>`, i.e. **two nested
  `Result`s** plus the `Option`. Match it as `Ok(Ok(Some(paths)))`:

```rust
let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
    files: false,
    directories: true,
    multiple: false,
    prompt: None,             // 4th field! Option<SharedString> = dialog confirm label
});
cx.spawn(async move |this, cx| {
    if let Ok(Ok(Some(paths))) = paths.await {
        if let Some(p) = paths.first() {
            let p = p.display().to_string();
            let _ = this.update(cx, |this, cx| { this.picked = p.into(); cx.notify(); });
        }
    }
})
.detach();
```

- **`PathPromptOptions`** (`src/platform.rs:1330`) — full field list:
  ```rust
  pub struct PathPromptOptions {
      pub files: bool,
      pub directories: bool,
      pub multiple: bool,
      pub prompt: Option<SharedString>,  // NOT in the draft spec
  }
  ```
  It is `#[derive(Clone, Debug)]` but **not** `Default`, so all 4 fields must be
  given explicitly.

- There is also `App::prompt_for_new_path(&self, directory: &Path,
  suggested_name: Option<&str>) -> oneshot::Receiver<Result<Option<PathBuf>>>`
  (a save dialog) if a "choose output file" flow is needed later.

## (d) `cx.listener` signature

`Context::listener` (`src/app/context.rs:252`):

```rust
pub fn listener<E: ?Sized>(
    &self,
    f: impl Fn(&mut T, &E, &mut Window, &mut Context<T>) + 'static,
) -> impl Fn(&E, &mut Window, &mut App) + 'static
```

- The closure you write takes **4 args, in this order**:
  `|this: &mut T, event: &E, window: &mut Window, cx: &mut Context<T>|`.
- It adapts to the raw event-handler shape gpui wants
  (`Fn(&E, &mut Window, &mut App)`), so pass `cx.listener(...)` directly to
  handlers like `on_click`.

For `on_click` specifically (`InteractiveElement::on_click`, `src/elements/div.rs:1117`):
- Signature: `on_click(self, listener: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self`.
- **Requires a stateful element**, so call `.id("...")` on the `div()` first
  (`div().id("pick")` -> `Stateful<Div>`) before `.on_click(...)`.

```rust
div()
    .id("pick")
    .child("Pick a directory")
    .on_click(cx.listener(|this, _event: &gpui::ClickEvent, _window, cx| {
        // this: &mut Spike, cx: &mut Context<Spike>
    }))
```

## (e) `cx.spawn` signature

`Context::spawn` (`src/app/context.rs:237`):

```rust
pub fn spawn<AsyncFn, R>(&self, f: AsyncFn) -> Task<R>
where
    T: 'static,
    AsyncFn: AsyncFnOnce(WeakEntity<T>, &mut AsyncApp) -> R + 'static,
    R: 'static,
```

- The closure is an **`AsyncFnOnce`** taking **2 args**:
  `(WeakEntity<T>, &mut AsyncApp)`. Write it as
  **`async move |this, cx| { ... }`** (the `async ||` closure form), NOT the
  `|this, mut cx| async move { ... }` form.
  - `this: WeakEntity<Self>` — use `this.update(cx, |this, cx| ...)`, which
    returns `Result<_>` (errors if the entity was dropped).
  - `cx: &mut AsyncApp` — held across await points.
- Returns a `Task<R>`. **Must be retained or `.detach()`ed**, otherwise the
  task is cancelled when the `Task` drops.

(Also available: `App::spawn(async move |cx: &mut AsyncApp| ...)` without the
weak-entity arg, `src/app.rs:1417`; and `Context::spawn_in` /
`Window::spawn` variants that also thread a window. The plain `Context::spawn`
above is what the spike uses.)

---

## Known-good APIs reused from the hello-world example

Confirmed already-building, unchanged from `examples/rust/gpui_hello_world`:

- `Application::new().run(|cx: &mut App| { ... })`
- `Bounds::centered(None, size(px(w), px(h)), cx)`
- `cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() }, |_, cx| cx.new(|cx| State::new(cx)))`
  - **Gotcha:** `cx.new(...)` is a method of the **`AppContext` trait**. In a
    file that does not `use gpui::prelude::*`, you must `use gpui::AppContext;`
    explicitly (the hello-world only worked because it imports the prelude).
- `cx.activate(true)`
- `div()` + styling chain: `.flex()`, `.flex_col()`, `.bg(rgb(0x..))`,
  `.size_full()`, `.text_color(rgb(0x..))`, `.justify_center()`,
  `.items_center()`, `.child(...)`. `impl Render for State { fn render(&mut self,
  _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement { ... } }`.
- `.child(...)` accepts `String`/`&str` (via `IntoElement`) and child elements.
