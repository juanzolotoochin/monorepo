"""Collect launcher assets and their transitive runfiles."""

def _runfile(file):
    return file.short_path[3:] if file.short_path.startswith("../") else "_main/" + file.short_path

def _assets_impl(ctx):
    out = ctx.actions.declare_file(ctx.label.name + ".json")
    values = {
        "quantization": ctx.attr.quantization,
        "engine": _runfile(ctx.file.engine),
        "server": _runfile(ctx.attr.server[DefaultInfo].files_to_run.executable),
        "opencode": _runfile(ctx.file.opencode) + "/opencode",
        "pack": _runfile(ctx.file.pack),
        "draft": _runfile(ctx.file.draft),
        "shards": [_runfile(f) for f in ctx.files.shards],
        "profile": _runfile(ctx.file.profile),
        "ripgrep": _runfile(ctx.file.ripgrep),
        "libraries": sorted({_runfile(f).rpartition("/")[0]: True for f in ctx.files.libraries}.keys()),
    }
    ctx.actions.write(out, json.encode_indent(values) + "\n")
    targets = [
        ctx.attr.engine,
        ctx.attr.server,
        ctx.attr.opencode,
        ctx.attr.pack,
        ctx.attr.draft,
        ctx.attr.profile,
        ctx.attr.ripgrep,
    ] + ctx.attr.shards + ctx.attr.libraries
    runfiles = ctx.runfiles(
        files = [out],
        transitive_files = depset(transitive = [target[DefaultInfo].files for target in targets]),
    )
    for target in targets:
        runfiles = runfiles.merge(target[DefaultInfo].default_runfiles)
    return [DefaultInfo(files = depset([out]), runfiles = runfiles)]

launcher_assets = rule(
    implementation = _assets_impl,
    attrs = {
        "quantization": attr.string(mandatory = True, values = ["IQ2_XS", "IQ3_XXS"]),
        "engine": attr.label(allow_single_file = True, mandatory = True),
        "server": attr.label(executable = True, cfg = "target", mandatory = True),
        "opencode": attr.label(allow_single_file = True, mandatory = True),
        "pack": attr.label(allow_single_file = True, mandatory = True),
        "draft": attr.label(allow_single_file = True, mandatory = True),
        "profile": attr.label(allow_single_file = True, mandatory = True),
        "ripgrep": attr.label(allow_single_file = True, mandatory = True),
        "shards": attr.label_list(allow_files = True),
        "libraries": attr.label_list(allow_files = True),
    },
)
