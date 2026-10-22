"""Pinned NVIDIA CUDA redistribution components."""

def _cuda_impl(ctx):
    components = json.decode(ctx.read(ctx.attr.lock))
    for component in components.values():
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

_cuda_repository = repository_rule(
    implementation = _cuda_impl,
    attrs = {"lock": attr.label(mandatory = True, allow_single_file = True)},
)

def cuda_repositories():
    """Declare the CUDA SDK and runtime repository."""
    _cuda_repository(
        name = "localllm_cuda",
        lock = Label("//localllm/cuda:components.lock.json"),
    )
