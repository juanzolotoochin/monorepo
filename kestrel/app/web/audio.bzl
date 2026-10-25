"""Build Kestrel's procedural audio cues as browser assets."""

_CUE_COUNT = 20

def _web_audio_impl(ctx):
    cues = [ctx.actions.declare_file(ctx.label.name + "/cue_%d.wav" % i) for i in range(_CUE_COUNT)]
    manifest = ctx.actions.declare_file(ctx.label.name + "/cues.json")
    ctx.actions.run(
        executable = ctx.attr._generator[DefaultInfo].files_to_run,
        arguments = [manifest.dirname, str(_CUE_COUNT)],
        outputs = cues + [manifest],
        mnemonic = "KestrelWebAudio",
        progress_message = "Generating browser audio %{label}",
    )
    return [DefaultInfo(files = depset(cues + [manifest]))]

web_audio = rule(
    implementation = _web_audio_impl,
    attrs = {
        "_generator": attr.label(default = "//kestrel/app/web:generate_audio", executable = True, cfg = "exec"),
    },
)
