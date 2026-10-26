# Bazel builds

- Use `--config=remote` for Bazel builds of Kestrel targets, including the build step of `bazel test` and `bazel run`. For example: `bazel build --config=remote //kestrel/app:demo` and `bazel test --config=remote //kestrel/app:movement_test`.
- Run tests with `bazel test --config=remote`, not `bazel run` on a test target or its binary. This keeps execution under Bazel's test runner, including its test environment, results, and cache. Use `bazel run` for applications and development tools.
- Keep the repository's `.bazelrc` defaults. Avoid ad hoc `--platforms`, `--host_platform`, `--extra_execution_platforms`, `--action_env`, `--repo_env`, compiler, linker, or configuration flags that change action keys and bypass the shared remote cache. Add a flag only when the target or task requires it, and explain why.
- Some manual GPU/video targets require local execution. For those documented exceptions, use their local workflow rather than forcing a remote build.

# Task list

- Deferred work lives in `kestrel/todo/`, one file per topic (`networking.md`, ...). The format is in `kestrel/todo/README.md`.
- When you find something worth doing later that is out of scope for the current task, add it there instead of leaving it only in a chat, a PR description or a handoff.
- When you finish a listed task, mark it done and move it to the topic's `## Done` section with a link to the PR.
