# Windows shell toolchain

Windows `sh_binary` and `sh_test` launchers use the checksum-pinned MSYS2 base
archive declared in `MODULE.bazel`. Bash, its DLLs, and supporting utilities are
declared runfiles, so no Git Bash installation, `BAZEL_SH`, or global PATH
changes are required. The native Bazel launcher resolves Bash through the
runfiles manifest and puts its directory first on PATH.

The small `rules_shell` patch merges the toolchain's runtime runfiles into shell
targets. Registration is restricted to Windows x86-64 targets and execution
platforms; Linux remote workers retain their Linux shell toolchain.

Validate with `bazelisk test //bazel/windows_shell:smoke_test`.

This supplies the `rules_shell` toolchain, including the shell launcher Bazel
analyzes for coverage support. It does not change Bazel's separate shell
selection for native genrules. Prefer declared executable tools over shell
commands for portable build actions.
