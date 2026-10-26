# Bazel builds

- Use `--config=remote` for Bazel builds of Kestrel targets, including the build step of `bazel test` and `bazel run`. For example: `bazel build --config=remote //kestrel/app:demo` and `bazel test --config=remote //kestrel/app:movement_test`.
- Run tests with `bazel test --config=remote`, not `bazel run` on a test target or its binary. This keeps execution under Bazel's test runner, including its test environment, results, and cache. Use `bazel run` for applications and development tools.
- Keep the repository's `.bazelrc` defaults. Avoid ad hoc `--platforms`, `--host_platform`, `--extra_execution_platforms`, `--action_env`, `--repo_env`, compiler, linker, or configuration flags that change action keys and bypass the shared remote cache. Add a flag only when the target or task requires it, and explain why.
- Some manual GPU/video targets require local execution. For those documented exceptions, use their local workflow rather than forcing a remote build.

# Task list

- Deferred work lives in `kestrel/todo/`, one file per topic (`networking.md`, ...). The format is in `kestrel/todo/README.md`.
- When you find something worth doing later that is out of scope for the current task, add it there instead of leaving it only in a chat, a PR description or a handoff.
- When you finish a listed task, mark it done and move it to the topic's `## Done` section with a link to the PR.

# Test time

We track how much of a task is spent waiting on Bazel, to decide whether
the build and test loop is worth optimizing.

- When you start a task, note the time (`date`).
- For every `bazel test`, `bazel build` and `bazel run` you wait on, add up the `INFO: Elapsed time: Ns` line Bazel prints at the end. Count failed and repeated runs too.
- When the task ends, add one row to `kestrel/docs/test-time.md`: the date, the task, the total time from start to finish, the time spent in Bazel, the number of Bazel runs, and a short note (e.g. "golden regenerated twice", "remote cache cold").
- Say the two numbers in your final report too.
