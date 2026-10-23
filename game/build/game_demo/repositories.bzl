"""Explicit discovery of the local video encoder used by GPU demo actions."""

def _tools_impl(ctx):
    encoder = ctx.os.environ.get("GAME_DEMO_FFMPEG") or ctx.which("ffmpeg")
    if not encoder:
        fail("game_demo requires local FFmpeg; install it or set --repo_env=GAME_DEMO_FFMPEG=/absolute/path")
    ctx.symlink(encoder, "ffmpeg")
    ctx.file("BUILD.bazel", 'exports_files(["ffmpeg"], visibility = ["//visibility:public"])\n')

local_demo_tools = repository_rule(
    implementation = _tools_impl,
    configure = True,
    local = True,
    environ = ["PATH", "GAME_DEMO_FFMPEG"],
)
