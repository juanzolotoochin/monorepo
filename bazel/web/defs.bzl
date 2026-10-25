"""Reusable Bazel packaging and local serving for Rust WebAssembly apps."""

load("@rules_python//python:defs.bzl", "py_binary")

def _wasm_platform_impl(_settings, _attr):
    return {"//command_line_option:platforms": ["@rules_rs//rs/platforms:wasm32-unknown-unknown"]}

_wasm_platform = transition(
    implementation = _wasm_platform_impl,
    inputs = [],
    outputs = ["//command_line_option:platforms"],
)

def _wasm_web_bundle_impl(ctx):
    game = ctx.file.game
    if not game.basename.endswith(".wasm"):
        fail("game must produce a .wasm file: " + game.path)
    stem = game.basename[:-5]
    directory = ctx.label.name
    javascript = ctx.actions.declare_file(directory + "/" + stem + ".js")
    wasm = ctx.actions.declare_file(directory + "/" + stem + "_bg.wasm")
    index = ctx.actions.declare_file(directory + "/index.html")

    ctx.actions.run(
        executable = ctx.file._bindgen,
        arguments = [
            game.path,
            "--target", "web",
            "--no-typescript",
            "--remove-name-section",
            "--out-dir", javascript.dirname,
        ],
        inputs = [game],
        outputs = [javascript, wasm],
        mnemonic = "WasmBindgen",
        progress_message = "Packaging browser app %{label}",
    )
    ctx.actions.expand_template(
        template = ctx.file.index_template,
        output = index,
        substitutions = {"{{GAME_JS}}": javascript.basename},
    )

    assets = []
    names = {javascript.basename: True, wasm.basename: True, index.basename: True}
    for source in ctx.files.assets:
        if source.basename in names:
            fail("duplicate browser asset name: " + source.basename)
        names[source.basename] = True
        output = ctx.actions.declare_file(directory + "/" + source.basename)
        ctx.actions.symlink(output = output, target_file = source)
        assets.append(output)
    return [DefaultInfo(files = depset([javascript, wasm, index] + assets))]

wasm_web_bundle = rule(
    implementation = _wasm_web_bundle_impl,
    attrs = {
        "game": attr.label(allow_single_file = True, cfg = _wasm_platform, mandatory = True),
        "index_template": attr.label(allow_single_file = True, mandatory = True),
        "assets": attr.label_list(allow_files = True),
        "_bindgen": attr.label(default = "@wasm_bindgen_cli//:wasm-bindgen", allow_single_file = True, cfg = "exec"),
        "_allowlist_function_transition": attr.label(default = "@bazel_tools//tools/allowlists/function_transition_allowlist"),
    },
)

def wasm_web_app(name, game, index_template, assets = [], visibility = None):
    """Expose a WASM bundle and `bazel run` devserver in the caller's package."""
    bundle = name + "_bundle"
    package = native.package_name()
    bundle_path = package + "/" + bundle if package else bundle
    wasm_web_bundle(
        name = bundle,
        game = game,
        index_template = index_template,
        assets = assets,
        visibility = visibility,
    )
    py_binary(
        name = name,
        srcs = ["//bazel/web:serve.py"],
        main = "//bazel/web:serve.py",
        data = [":" + bundle],
        deps = ["@rules_python//python/runfiles"],
        args = ["--bundle", bundle_path],
        visibility = visibility,
    )
