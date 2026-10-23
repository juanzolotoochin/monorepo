"""Extract a link-time shared library using a declared Python tool."""

def _extract_library_impl(ctx):
    output = ctx.actions.declare_file("lib/lib%s.so" % ctx.attr.library)
    ctx.actions.run(
        executable = ctx.attr._extractor[DefaultInfo].files_to_run,
        arguments = [ctx.file.archive.path, ctx.attr.soname, output.path],
        inputs = [ctx.file.archive],
        outputs = [output],
        mnemonic = "ExtractSharedLibrary",
        progress_message = "Extracting shared library %{label}",
    )
    return [DefaultInfo(files = depset([output]))]

extract_library = rule(
    implementation = _extract_library_impl,
    attrs = {
        "archive": attr.label(allow_single_file = True, mandatory = True),
        "library": attr.string(mandatory = True),
        "soname": attr.string(mandatory = True),
        "_extractor": attr.label(
            default = "//third_party/x11:extract_library_tool",
            executable = True,
            cfg = "exec",
        ),
    },
)
