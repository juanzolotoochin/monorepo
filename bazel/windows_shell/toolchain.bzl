"""Runfiles-aware Windows shell toolchain using a pinned MSYS2 runtime."""

def _windows_shell_impl(ctx):
    shell = ctx.file.shell

    # Bazel's native Bash launcher resolves this through its runfiles manifest.
    path = shell.short_path[3:] if shell.short_path.startswith("../") else ctx.workspace_name + "/" + shell.short_path
    return [platform_common.ToolchainInfo(
        path = path,
        launcher = ctx.executable.launcher,
        launcher_maker = ctx.executable.launcher_maker,
        runtime_runfiles = ctx.runfiles(files = ctx.files.runtime),
    )]

windows_shell = rule(
    implementation = _windows_shell_impl,
    attrs = {
        "shell": attr.label(allow_single_file = True, mandatory = True),
        "runtime": attr.label(mandatory = True),
        "launcher": attr.label(
            default = "@bazel_tools//tools/launcher:launcher",
            executable = True,
            cfg = "target",
        ),
        "launcher_maker": attr.label(
            default = "@bazel_tools//tools/launcher:launcher_maker",
            executable = True,
            cfg = "exec",
        ),
    },
)
