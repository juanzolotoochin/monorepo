"""Pinned dependencies for the local inference applications.

Repository declarations stay here; compilation and model conversion remain
ordinary build actions. Repositories are fetched only when targets need them.
"""

load("//localllm/cuda:repositories.bzl", "cuda_repositories")
load("//localllm/opencode:repositories.bzl", "opencode_repositories")
load("//localllm/strata:repositories.bzl", "strata_repositories")

def _dependencies_impl(ctx):
    cuda_repositories()
    strata_repositories()
    opencode_repositories()
    return ctx.extension_metadata(
        root_module_direct_deps = "all",
        root_module_direct_dev_deps = [],
        reproducible = True,
    )

dependencies = module_extension(implementation = _dependencies_impl)
