"""Build the pinned terminal client without runtime package installation."""

def _opencode_binary_impl(ctx):
    out = ctx.actions.declare_directory(ctx.label.name + ".dist")
    args = ctx.actions.args()
    args.add(ctx.file._script)
    args.add(ctx.file.package)
    args.add(out.path)
    args.add(ctx.file._lock)
    ctx.actions.run(
        executable = ctx.executable.bun,
        arguments = [args],
        inputs = depset(ctx.files.srcs + [ctx.file._script, ctx.file._lock]),
        tools = [ctx.attr.bun[DefaultInfo].files_to_run],
        outputs = [out],
        env = {"SOURCE_DATE_EPOCH": "0", "TZ": "UTC", "LC_ALL": "C", "HOME": "/nonexistent"},
        mnemonic = "BuildOpenCode",
        progress_message = "Building pinned OpenCode with Bun (offline)",
    )
    return [DefaultInfo(files = depset([out]))]

opencode_binary = rule(
    implementation = _opencode_binary_impl,
    attrs = {
        "srcs": attr.label_list(allow_files = True),
        "package": attr.label(allow_single_file = True),
        "bun": attr.label(executable = True, cfg = "exec"),
        "_script": attr.label(default = "//localllm/opencode:build.ts", allow_single_file = True),
        "_lock": attr.label(default = "//localllm/opencode:npm.lock.json", allow_single_file = True),
    },
)
