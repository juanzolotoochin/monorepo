"""Network-free build actions for the local inference applications."""

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
        "_script": attr.label(default = "//localllm/private:build_opencode.ts", allow_single_file = True),
        "_lock": attr.label(default = "//localllm/private:opencode-npm.lock.json", allow_single_file = True),
    },
)

def _model_pack_impl(ctx):
    out = ctx.actions.declare_directory(ctx.label.name)
    ctx.actions.run(
        executable = ctx.executable.tool,
        arguments = ["--gguf", ctx.file.first.path, "--out", out.path],
        inputs = depset(ctx.files.shards),
        tools = [ctx.attr.tool[DefaultInfo].files_to_run],
        outputs = [out],
        env = {"PYTHONHASHSEED": "0", "TZ": "UTC", "LC_ALL": "C"},
        mnemonic = "PrepareStrataModel",
        progress_message = "Preparing the pinned model and tokenizer (offline)",
    )
    return [DefaultInfo(files = depset([out]))]

model_pack = rule(
    implementation = _model_pack_impl,
    attrs = {
        "tool": attr.label(executable = True, cfg = "exec", default = "@localllm_strata//:pack"),
        "first": attr.label(allow_single_file = True, mandatory = True),
        "shards": attr.label_list(allow_files = True, mandatory = True),
    },
)

def _draft_pack_impl(ctx):
    out = ctx.actions.declare_directory(ctx.label.name)
    ctx.actions.run(
        executable = ctx.executable.tool,
        arguments = [ctx.file.manifest.path, out.path, ctx.file.vocab.path],
        inputs = depset(ctx.files.tensors + [ctx.file.vocab]),
        tools = [ctx.attr.tool[DefaultInfo].files_to_run],
        outputs = [out],
        env = {
            "PYTHONHASHSEED": "0",
            "TZ": "UTC",
            "LC_ALL": "C",
            "OPENBLAS_NUM_THREADS": "1",
            "OMP_NUM_THREADS": "1",
            "NPY_DISABLE_CPU_FEATURES": "AVX,F16C,FMA3,AVX2,AVX512F,AVX512_SKX,AVX512_CLX,AVX512_CNL,AVX512_ICL,AVX512_SPR",
        },
        mnemonic = "PrepareStrataDraft",
        progress_message = "Quantizing the pinned draft model (offline)",
    )
    return [DefaultInfo(files = depset([out]))]

draft_pack = rule(
    implementation = _draft_pack_impl,
    attrs = {
        "tool": attr.label(executable = True, cfg = "exec", mandatory = True),
        "manifest": attr.label(allow_single_file = True, mandatory = True),
        "tensors": attr.label_list(allow_files = True, mandatory = True),
        "vocab": attr.label(allow_single_file = True, mandatory = True),
    },
)

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
    targets = [ctx.attr.engine, ctx.attr.server, ctx.attr.opencode, ctx.attr.pack, ctx.attr.draft, ctx.attr.profile, ctx.attr.ripgrep] + ctx.attr.shards + ctx.attr.libraries
    runfiles = ctx.runfiles(files = [out] + [f for t in targets for f in t[DefaultInfo].files.to_list()])
    for target in targets:
        runfiles = runfiles.merge(target[DefaultInfo].default_runfiles)
    return [DefaultInfo(files = depset([out]), runfiles = runfiles)]

launcher_assets = rule(
    implementation = _assets_impl,
    attrs = {
        "quantization": attr.string(mandatory = True, values = ["IQ2_XS", "IQ3_XXS"]),
        "engine": attr.label(allow_single_file = True),
        "server": attr.label(executable = True, cfg = "target"),
        "opencode": attr.label(allow_single_file = True),
        "pack": attr.label(allow_single_file = True),
        "draft": attr.label(allow_single_file = True),
        "profile": attr.label(allow_single_file = True),
        "ripgrep": attr.label(allow_single_file = True),
        "shards": attr.label_list(allow_files = True),
        "libraries": attr.label_list(allow_files = True),
    },
)
