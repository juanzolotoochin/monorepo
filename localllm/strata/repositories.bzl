"""Pinned Strata/ggml sources and model inputs."""

def _source_impl(ctx):
    ctx.download_and_extract(
        url = ctx.attr.url,
        sha256 = ctx.attr.sha256,
        stripPrefix = ctx.attr.strip_prefix,
        type = "tar.gz",
    )
    for source in json.decode(ctx.read(ctx.attr.cuda_sources))[ctx.attr.source_key]:
        # rules_cc recognizes .cc; Clang's explicit -x cuda selects CUDA mode.
        # Adjacent aliases preserve upstream relative include paths.
        ctx.symlink(source, source + ".cc")
    for path, content in ctx.attr.generated_files.items():
        ctx.file(path, content)
    ctx.file("BUILD.bazel", ctx.read(ctx.attr.build_file))

_source_repository = repository_rule(
    implementation = _source_impl,
    attrs = {
        "url": attr.string(mandatory = True),
        "sha256": attr.string(mandatory = True),
        "strip_prefix": attr.string(mandatory = True),
        "source_key": attr.string(mandatory = True),
        "cuda_sources": attr.label(mandatory = True, allow_single_file = True),
        "generated_files": attr.string_dict(),
        "build_file": attr.label(mandatory = True, allow_single_file = True),
    },
)

def _model_impl(ctx):
    manifest = json.decode(ctx.read(ctx.attr.lock))
    pending = []
    for artifact in manifest["files"]:
        pending.append(ctx.download(
            url = artifact["url"],
            output = artifact["name"],
            sha256 = artifact["sha256"],
            block = False,
        ))
    for download in pending:
        download.wait()
    for artifact in manifest["files"]:
        # Download checksums already validate contents; size is recorded for review.
        if not ctx.path(artifact["name"]).exists:
            fail("Missing model artifact: " + artifact["name"])
    ctx.file("BUILD.bazel", """
package(default_visibility = ["//visibility:public"])
filegroup(name = "files", srcs = glob(["*.gguf"]))
exports_files(glob(["*.gguf"]))
""")

_model_repository = repository_rule(
    implementation = _model_impl,
    attrs = {"lock": attr.label(mandatory = True, allow_single_file = True)},
)

def _mtp_impl(ctx):
    manifest = json.decode(ctx.read(ctx.attr.lock))
    pending = []
    for tensor in manifest["tensors"]:
        pending.append(ctx.download(
            url = manifest["repo"] + tensor["shard"],
            output = tensor["file"],
            sha256 = tensor["sha256"],
            headers = {"Range": ["bytes={}-{}".format(tensor["start"], tensor["end"])]},
            block = False,
        ))
    for download in pending:
        download.wait()
    ctx.file("mtp-manifest.json", json.encode_indent(manifest["tensors"]) + "\n")
    ctx.file("BUILD.bazel", """
package(default_visibility = ["//visibility:public"])
exports_files(["mtp-manifest.json"])
filegroup(name = "files", srcs = glob(["tensors/*.bin"]) + ["mtp-manifest.json"])
""")

_mtp_repository = repository_rule(
    implementation = _mtp_impl,
    attrs = {"lock": attr.label(mandatory = True, allow_single_file = True)},
)

def strata_repositories():
    """Declare inference sources and both lazily fetched model variants."""
    _source_repository(
        name = "localllm_strata",
        build_file = Label("//localllm/strata:strata.BUILD.bazel"),
        cuda_sources = Label("//localllm/cuda:sources.json"),
        sha256 = "c043e9f4c55fef75bfffeed2c034b3c91eb02e5e5fca44739839baca58f961ce",
        source_key = "strata",
        strip_prefix = "Strata-6f32ec070f23ced9f50e704d854d775da52591ab",
        url = "https://codeload.github.com/Niko1221/Strata/tar.gz/6f32ec070f23ced9f50e704d854d775da52591ab",
    )

    _source_repository(
        name = "localllm_ggml",
        build_file = Label("//localllm/strata:ggml.BUILD.bazel"),
        cuda_sources = Label("//localllm/cuda:sources.json"),
        sha256 = "c076d7534afa0e5d0ec2a0d425b11e791c16f3de0d727221aea071cef156a280",
        source_key = "ggml",
        generated_files = {
            "ggml/src/ggml-version.h": '#define GGML_VERSION "0.24.0"\n#define GGML_COMMIT "3cf03257f219afbe7334045ff7c6a06ac68c627d"\n',
        },
        strip_prefix = "llama.cpp-3cf03257f219afbe7334045ff7c6a06ac68c627d",
        url = "https://codeload.github.com/ggml-org/llama.cpp/tar.gz/3cf03257f219afbe7334045ff7c6a06ac68c627d",
    )

    _model_repository(
        name = "localllm_model",
        lock = Label("//localllm/strata:model-iq2-xs.lock.json"),
    )
    _model_repository(
        name = "localllm_model_iq3_xxs",
        lock = Label("//localllm/strata:model-iq3-xxs.lock.json"),
    )
    _mtp_repository(
        name = "localllm_mtp",
        lock = Label("//localllm/strata:mtp.lock.json"),
    )
