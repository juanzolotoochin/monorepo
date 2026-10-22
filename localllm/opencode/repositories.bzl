"""Pinned OpenCode sources, npm closure, and executable tools."""

load("@bazel_tools//tools/build_defs/repo:http.bzl", "http_archive")

def _opencode_impl(ctx):
    # OpenCode v2.0.32, pinned to the release commit.
    ctx.download_and_extract(
        url = "https://codeload.github.com/anomalyco/opencode/tar.gz/843a6c514e606b436a7fe713422ace0a93ad3d61",
        sha256 = "77a226054bc40f787a4fe8b8a4d420fdddc45c68b5800e6121c753a16f40acad",
        stripPrefix = "opencode-843a6c514e606b436a7fe713422ace0a93ad3d61",
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
            if "patch" in package:
                # Apply upstream's Bun dependency patches without running an installer.
                prefix = unpack + "/" + roots[0].basename

                # Bun also records empty package-manager marker files, which
                # carry no source changes and aren't supported by Bazel's patcher.
                patch = "".join([
                    "diff --git " + diff
                    for diff in ctx.read(package["patch"]).split("diff --git ")
                    if "\n@@ " in diff
                ])
                patch = patch.replace("\n--- a/", "\n--- a/" + prefix + "/")
                patch = patch.replace("\n+++ b/", "\n+++ b/" + prefix + "/")
                ctx.file("dependency.patch", patch)
                ctx.patch("dependency.patch", strip = 1)
                ctx.delete("dependency.patch")
            ctx.symlink(roots[0], package["dest"])
            ctx.delete(archive)
    ctx.file("BUILD.bazel", ctx.read(ctx.attr.build_file))

_opencode_repository = repository_rule(
    implementation = _opencode_impl,
    attrs = {
        "lock": attr.label(mandatory = True, allow_single_file = True),
        "build_file": attr.label(mandatory = True, allow_single_file = True),
        "patch": attr.label(mandatory = True, allow_single_file = True),
    },
)

def opencode_repositories():
    """Declare the terminal client and its pinned tools."""
    _opencode_repository(
        name = "localllm_opencode",
        build_file = Label("//localllm/opencode:upstream.BUILD.bazel"),
        lock = Label("//localllm/opencode:npm.lock.json"),
        patch = Label("//localllm/opencode:offline.patch"),
    )

    http_archive(
        name = "localllm_bun",
        build_file_content = 'load("@bazel_skylib//rules:native_binary.bzl", "native_binary")\nnative_binary(name = "bun_tool", src = "bun", out = "bun-bin", visibility = ["//visibility:public"])',
        sha256 = "36368faef7527875d5ffa52e53cd48021741f2a83eb6208a8dd64068d422a913",
        strip_prefix = "bun-linux-x64",
        urls = ["https://github.com/oven-sh/bun/releases/download/bun-v1.4.2/bun-linux-x64.zip"],
    )

    http_archive(
        name = "localllm_ripgrep",
        build_file_content = 'exports_files(["rg"], visibility = ["//visibility:public"])',
        sha256 = "1c9297be4a084eea7ecaedf93eb03d058d6faae29bbc57ecdaf5063921491599",
        strip_prefix = "ripgrep-15.1.0-x86_64-unknown-linux-musl",
        urls = ["https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz"],
    )
