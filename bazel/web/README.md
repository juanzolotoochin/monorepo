# Rust WebAssembly apps

`wasm_web_app` in `defs.bzl` creates a browser bundle and a local server. It is
shared build infrastructure; game-specific HTML, JavaScript, and assets stay in
the game's package.

```starlark
wasm_web_app(
    name = "web",
    game = ":game_binary",
    index_template = ":index.html.tpl",
    assets = [":web_assets"],
)
```

The game binary is compiled for `wasm32-unknown-unknown` while the server and
build tools run on the host. The rule packages the binary with `wasm-bindgen`,
replaces `{{GAME_JS}}` in the HTML template with the generated module name, and
places assets at the bundle root. Asset basenames must be unique. Run
`bazel run //path/to/game:web` and open the printed URL. The server binds to
localhost; pass `-- --port 8080` to use another port.

The pinned `wasm-bindgen` CLI currently runs on Linux x86-64 and must match the
`wasm-bindgen` Rust crate version in `Cargo.lock`.
