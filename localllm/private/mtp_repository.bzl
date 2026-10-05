"""Fetch only pinned draft tensors, validating each range by SHA-256."""

def _mtp_impl(ctx):
    manifest = json.decode(ctx.read(ctx.attr.lock))
    pending = []
    for tensor in manifest["tensors"]:
        pending.append(ctx.download(
            url = manifest["repo"] + tensor["shard"],
            output = tensor["file"],
            sha256 = tensor["sha256"],
            headers = {"Range": ["bytes={}-{}".format(tensor["start"], tensor["end"])]},
            block = False,
        ))
    for download in pending:
        download.wait()
    ctx.file("mtp-manifest.json", json.encode_indent(manifest["tensors"]) + "\n")
    ctx.file("BUILD.bazel", """
package(default_visibility = ["//visibility:public"])
exports_files(["mtp-manifest.json"])
filegroup(name = "files", srcs = glob(["tensors/*.bin"]) + ["mtp-manifest.json"])
""")

mtp_repository = repository_rule(
    implementation = _mtp_impl,
    attrs = {"lock": attr.label(mandatory = True, allow_single_file = True)},
)
