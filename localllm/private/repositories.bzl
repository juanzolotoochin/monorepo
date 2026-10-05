"""Pinned local inference dependencies. Repository rules only fetch verified bytes."""

def _cuda_impl(ctx):
    components = json.decode(ctx.read(ctx.attr.lock))
    for name, component in components.items():
        path = component["relative_path"]
        ctx.download_and_extract(
            url = "https://developer.download.nvidia.com/compute/cuda/redist/" + path,
            sha256 = component["sha256"],
            stripPrefix = path.split("/")[-1].removesuffix(".tar.xz"),
        )
    ctx.file("BUILD.bazel", """
load("@rules_cc//cc:defs.bzl", "cc_import", "cc_library")
package(default_visibility = ["//visibility:public"])
filegroup(name = "files", srcs = glob(["**"], exclude = ["BUILD.bazel"]))
exports_files(["bin/nvcc"])
filegroup(name = "runtime", srcs = glob(["lib/*.so*"]))
cc_library(name = "headers", hdrs = glob(["include/**"]), includes = ["include"])
cc_import(name = "cudart", shared_library = "lib/libcudart.so.13")
cc_import(name = "cublas", shared_library = "lib/libcublas.so.13")
cc_import(name = "cublaslt", shared_library = "lib/libcublasLt.so.13")
cc_library(name = "runtime_libs", deps = [":cudart", ":cublas", ":cublaslt"])
""")

cuda_repository = repository_rule(
    implementation = _cuda_impl,
    attrs = {"lock": attr.label(mandatory = True, allow_single_file = True)},
)

def _source_impl(ctx):
    ctx.download_and_extract(url = ctx.attr.url, sha256 = ctx.attr.sha256, stripPrefix = ctx.attr.strip_prefix, type = "tar.gz")
    for source in json.decode(ctx.read(ctx.attr.cuda_sources))[ctx.attr.source_key]:
        # rules_cc recognizes .cc; Clang's explicit -x cuda selects CUDA mode.
        # Adjacent aliases preserve upstream relative include paths.
        ctx.symlink(source, source + ".cc")
    if ctx.attr.source_key == "ggml":
        ctx.file("ggml/src/ggml-version.h", '#define GGML_VERSION "0.24.0"\n#define GGML_COMMIT "3cf03257f219afbe7334045ff7c6a06ac68c627d"\n')
    ctx.file("BUILD.bazel", ctx.read(ctx.attr.build_file))

source_repository = repository_rule(
    implementation = _source_impl,
    attrs = {
        "url": attr.string(mandatory = True),
        "sha256": attr.string(mandatory = True),
        "strip_prefix": attr.string(mandatory = True),
        "source_key": attr.string(mandatory = True),
        "cuda_sources": attr.label(mandatory = True, allow_single_file = True),
        "build_file": attr.label(mandatory = True, allow_single_file = True),
    },
)

def _opencode_impl(ctx):
    ctx.download_and_extract(
        url = "https://codeload.github.com/anomalyco/opencode/tar.gz/907b3bc518fa48e90e8ec24dd327d13eee71c36c",
        sha256 = "d87cae271931495e1dbcce3c5a035a99de28af401800dcd1bc0c93c34f381600",
        stripPrefix = "opencode-907b3bc518fa48e90e8ec24dd327d13eee71c36c",
        type = "tar.gz",
    )
    ctx.patch(ctx.attr.patch, strip = 1)
    packages = json.decode(ctx.read(ctx.attr.lock))

    # Downloads use the Bun lockfile's SRI, without running npm lifecycle scripts.
    for start in range(0, len(packages), 32):
        pending = []
        for i in range(start, min(start + 32, len(packages))):
            package = packages[i]
            if "workspace" in package:
                ctx.symlink(package["workspace"], package["dest"])
            else:
                archive = "downloads/{}.tgz".format(i)
                pending.append((package, archive, ctx.download(
                    url = package["url"],
                    output = archive,
                    integrity = package["integrity"],
                    block = False,
                )))
        for package, archive, download in pending:
            download.wait()
            unpack = "npm_packages/{}".format(archive.split("/")[-1].removesuffix(".tgz"))
            ctx.extract(archive, output = unpack)
            roots = ctx.path(unpack).readdir()
            if len(roots) != 1 or not roots[0].is_dir:
                fail("Expected one root directory in {}".format(package["url"]))
            ctx.symlink(roots[0], package["dest"])
            ctx.delete(archive)
    ctx.file("BUILD.bazel", ctx.read(ctx.attr.build_file))

opencode_repository = repository_rule(
    implementation = _opencode_impl,
    attrs = {
        "lock": attr.label(mandatory = True, allow_single_file = True),
        "build_file": attr.label(mandatory = True, allow_single_file = True),
        "patch": attr.label(mandatory = True, allow_single_file = True),
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

model_repository = repository_rule(
    implementation = _model_impl,
    attrs = {"lock": attr.label(mandatory = True, allow_single_file = True)},
)
