"""Select a Rust target platform for binaries launched on the client."""

_PLATFORMS = {
    "linux-x86_64": Label("@rules_rs//rs/platforms:x86_64-unknown-linux-gnu"),
    "linux-aarch64": Label("@rules_rs//rs/platforms:aarch64-unknown-linux-gnu"),
    "macos-aarch64": Label("@rules_rs//rs/platforms:aarch64-apple-darwin"),
    "macos-x86_64": Label("@rules_rs//rs/platforms:x86_64-apple-darwin"),
    "windows-x86_64": Label("@rules_rs//rs/platforms:x86_64-pc-windows-msvc"),
}

def _host_platform_impl(ctx):
    os = "windows" if ctx.os.name.startswith("windows") else "macos" if ctx.os.name == "mac os x" else ctx.os.name
    arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(ctx.os.arch, ctx.os.arch)
    key = os + "-" + arch
    if key not in _PLATFORMS:
        fail("No native run platform configured for " + key)
    ctx.file("BUILD.bazel", """alias(
    name = "host",
    actual = "%s",
    visibility = ["//visibility:public"],
)
""" % _PLATFORMS[key])

host_platform = repository_rule(implementation = _host_platform_impl)
