"""Deterministic, offline preparation of Strata model artifacts."""

def _model_pack_impl(ctx):
    out = ctx.actions.declare_directory(ctx.label.name)
    ctx.actions.run(
        executable = ctx.executable.tool,
        arguments = ["--gguf", ctx.file.first.path, "--out", out.path],
        inputs = depset(ctx.files.shards + [ctx.file.first]),
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
        inputs = depset(ctx.files.tensors + [ctx.file.manifest, ctx.file.vocab]),
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
