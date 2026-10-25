# Bazel builds

- Use `--config=remote` for Bazel builds of Kestrel targets, including the build step of `bazel test` and `bazel run`. For example: `bazel build --config=remote //kestrel/app:demo` and `bazel test --config=remote //kestrel/app:movement_test`.
- Keep the repository's `.bazelrc` defaults. Avoid ad hoc `--platforms`, `--host_platform`, `--extra_execution_platforms`, `--action_env`, `--repo_env`, compiler, linker, or configuration flags that change action keys and bypass the shared remote cache. Add a flag only when the target or task requires it, and explain why.
- Some manual GPU/video targets require local execution. For those documented exceptions, use their local workflow rather than forcing a remote build.
