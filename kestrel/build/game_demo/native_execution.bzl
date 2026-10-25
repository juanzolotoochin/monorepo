"""Check the selected execution platform before running GPU tools locally."""

load("@platforms//host:constraints.bzl", "HOST_CONSTRAINTS")

NativeExecutionInfo = provider(fields = ["matches_host"])

def _native_execution_impl(ctx):
    return [NativeExecutionInfo(matches_host = all([
        ctx.target_platform_has_constraint(constraint[platform_common.ConstraintValueInfo])
        for constraint in ctx.attr._host_constraints
    ]))]

native_execution = rule(
    implementation = _native_execution_impl,
    attrs = {"_host_constraints": attr.label_list(default = HOST_CONSTRAINTS)},
)
