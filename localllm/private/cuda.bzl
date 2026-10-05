"""CUDA compilation with the monorepo's declared LLVM C++ toolchain."""

load("@rules_cc//cc:defs.bzl", "cc_library")

def cuda_library(name, srcs, deps, approximate = False):
    cc_library(
        name = name,
        srcs = srcs,
        copts = [
            "-x",
            "cuda",
            "--cuda-path=" + Label("@localllm_cuda//:files").workspace_root,
            "--cuda-gpu-arch=sm_89",
            "-std=c++20",
            "-O3",
            "-D_ALLOW_UNSUPPORTED_LIBCPP",
            "-Wno-unknown-cuda-version",
        ] + (["-fcuda-approx-transcendentals", "-fgpu-flush-denormals-to-zero", "-ffp-contract=fast"] if approximate else []),
        additional_compiler_inputs = ["@localllm_cuda//:files"],
        deps = deps,
    )
