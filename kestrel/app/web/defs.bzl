"""Build the browser bundle with explicit Bazel actions and a WASM game transition."""

def _wasm_platform_impl(_settings, _attr):
    return {"//command_line_option:platforms": ["@rules_rs//rs/platforms:wasm32-unknown-unknown"]}

_wasm_platform = transition(
    implementation = _wasm_platform_impl,
    inputs = [],
    outputs = ["//command_line_option:platforms"],
)

def _web_bundle_impl(ctx):
    directory = ctx.label.name
    javascript = ctx.actions.declare_file(directory + "/demo.js")
    wasm = ctx.actions.declare_file(directory + "/demo_bg.wasm")
    index = ctx.actions.declare_file(directory + "/index.html")
    cues = [ctx.actions.declare_file(directory + "/cue_%d.wav" % i) for i in range(20)]

    ctx.actions.run(
        executable = ctx.file._bindgen,
        arguments = [
            ctx.file.game.path,
            "--target", "web",
            "--no-typescript",
            "--remove-name-section",
            "--out-dir", javascript.dirname,
        ],
        inputs = [ctx.file.game],
        outputs = [javascript, wasm],
        mnemonic = "KestrelWasmBindgen",
        progress_message = "Packaging browser game %{label}",
    )
    ctx.actions.run(
        executable = ctx.attr._generate_audio[DefaultInfo].files_to_run,
        arguments = [javascript.dirname],
        outputs = cues,
        mnemonic = "KestrelWebAudio",
        progress_message = "Generating browser audio %{label}",
    )
    ctx.actions.expand_template(
        template = ctx.file._index_template,
        output = index,
        substitutions = {"{{GAME_JS}}": javascript.basename},
    )
    return [DefaultInfo(files = depset([javascript, wasm, index] + cues))]

web_bundle = rule(
    implementation = _web_bundle_impl,
    attrs = {
        "game": attr.label(allow_single_file = True, cfg = _wasm_platform, mandatory = True),
        "_index_template": attr.label(default = "//kestrel/app/web:index.html.tpl", allow_single_file = True),
        "_bindgen": attr.label(default = "@kestrel_wasm_bindgen//:wasm-bindgen", allow_single_file = True, cfg = "exec"),
        "_generate_audio": attr.label(default = "//kestrel/app/web:generate_audio", executable = True, cfg = "exec"),
        "_allowlist_function_transition": attr.label(default = "@bazel_tools//tools/allowlists/function_transition_allowlist"),
    },
)
