"""Build a WebAssembly binary while keeping its runnable server on the host."""

def _wasm_platform_impl(_settings, _attr):
    return {"//command_line_option:platforms": ["@rules_rs//rs/platforms:wasm32-unknown-unknown"]}

_wasm_platform = transition(
    implementation = _wasm_platform_impl,
    inputs = [],
    outputs = ["//command_line_option:platforms"],
)

def _wasm_binary_impl(ctx):
    return [DefaultInfo(files = depset([ctx.file.binary]))]

wasm_binary = rule(
    implementation = _wasm_binary_impl,
    attrs = {
        "binary": attr.label(allow_single_file = True, cfg = _wasm_platform, mandatory = True),
        "_allowlist_function_transition": attr.label(default = "@bazel_tools//tools/allowlists/function_transition_allowlist"),
    },
)
