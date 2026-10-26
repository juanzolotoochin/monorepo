"""Build Kestrel for Steam Frame while keeping the deploy tool on the host."""

def _arm64_platform_impl(_settings, _attr):
    return {"//command_line_option:platforms": ["@rules_rs//rs/platforms:aarch64-unknown-linux-gnu"]}

_arm64_platform = transition(
    implementation = _arm64_platform_impl,
    inputs = [],
    outputs = ["//command_line_option:platforms"],
)

def _arm64_game_impl(ctx):
    return [DefaultInfo(files = depset([ctx.file.game]))]

arm64_game = rule(
    implementation = _arm64_game_impl,
    attrs = {
        "game": attr.label(allow_single_file = True, cfg = _arm64_platform, mandatory = True),
        "_allowlist_function_transition": attr.label(default = "@bazel_tools//tools/allowlists/function_transition_allowlist"),
    },
)
