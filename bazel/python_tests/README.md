# Hermetic Python launchers

The repository enables rules_python's `bootstrap_impl=script` in `.bazelrc`.
Its Bash launcher starts the registered Python 3.11 runtime directly; host
`python` and `python3` are not needed to bootstrap Bazel Python binaries or tests.
Bash and the launcher's standard shell utilities remain platform prerequisites.
The setting also applies to execution tools and targets loading rules_python
without the repository's convenience macros.

`py_executable` preserves the executable's runtime runfiles instead of producing
a zipapp with a host-Python shebang. Distribute its executable together with its
`.runfiles` tree, just as for a normal `py_binary`.

Run the regression test with:

```
bazel test //bazel/python_tests:hermetic_bootstrap_test
```

The test places failing `python`, `python3`, and `python3.11` programs at the front
of PATH, then launches raw, wrapped, and exported Python binaries from an unrelated
working directory. It verifies the resolved interpreter and imports a declared
wheel dependency. It fails if a launcher invokes host Python.
