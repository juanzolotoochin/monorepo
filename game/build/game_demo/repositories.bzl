"""Pinned native encoders for local GPU demo actions."""

# Use a retained month-end snapshot; ordinary daily assets expire after 14 days.
_BASE = "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/"
_VERSION = "ffmpeg-n8.1.3-9-g29e619e767-"
_RELEASES = {
    "windows-x86_64": ("win64", ".zip", "7b801cdd3a1a0bb54ae6f572187e68b4ed54f52086cfee133fab2180bfb429fa"),
    "linux-x86_64": ("linux64", ".tar.xz", "97ce978979194b5cf7e06a5e68020dbdaa7a4f3294c5452b6a1bc347100dbb79"),
    "linux-aarch64": ("linuxarm64", ".tar.xz", "10523d1e0be6b3ce62a9708cc50815c4b5eb5f36ae147fbe2afeecda6f2d6845"),
}

def _tools_impl(ctx):
    windows = ctx.os.name.startswith("windows")
    binary = "bin/ffmpeg.exe" if windows else "bin/ffmpeg"
    override = ctx.os.environ.get("GAME_DEMO_FFMPEG")
    if override:
        ctx.symlink(override, binary)
    else:
        os = "windows" if windows else "linux" if ctx.os.name.startswith("linux") else "unsupported"
        arch = {"amd64": "x86_64", "arm64": "aarch64"}.get(ctx.os.arch, ctx.os.arch)
        key = os + "-" + arch
        if key not in _RELEASES:
            fail("No pinned FFmpeg for %s; set --repo_env=GAME_DEMO_FFMPEG=/absolute/path" % key)
        platform, extension, sha256 = _RELEASES[key]
        prefix = _VERSION + platform + "-gpl-8.1"
        ctx.download_and_extract(
            url = _BASE + prefix + extension,
            sha256 = sha256,
            stripPrefix = prefix,
        )
    ctx.file("BUILD.bazel", """package(default_visibility = ["//visibility:public"])
exports_files(["%s"])
alias(name = "ffmpeg", actual = "%s")
""" % (binary, binary))

local_demo_tools = repository_rule(
    implementation = _tools_impl,
    configure = True,
    local = True,
    environ = ["GAME_DEMO_FFMPEG"],
)
