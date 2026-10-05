"""Hermetic actions for the MIDI app's generated font and audio assets."""

def _run_python(ctx, args, inputs, outputs, mnemonic):
    ctx.actions.run(
        executable = ctx.attr._tool[DefaultInfo].files_to_run,
        arguments = [args],
        inputs = inputs,
        outputs = outputs,
        mnemonic = mnemonic,
    )

def _glyphs_impl(ctx):
    args = ctx.actions.args()
    args.add_all([ctx.file.music_font, ctx.file.lettering_font, ctx.outputs.music, ctx.outputs.lettering])
    _run_python(
        ctx,
        args,
        inputs = [ctx.file.music_font, ctx.file.lettering_font],
        outputs = [ctx.outputs.music, ctx.outputs.lettering],
        mnemonic = "MusicGlyphs",
    )
    return [DefaultInfo(files = depset([ctx.outputs.music, ctx.outputs.lettering]))]

music_glyphs = rule(
    implementation = _glyphs_impl,
    attrs = {
        "music_font": attr.label(allow_single_file = True, mandatory = True),
        "lettering_font": attr.label(allow_single_file = True, mandatory = True),
        "music": attr.output(mandatory = True),
        "lettering": attr.output(mandatory = True),
        "_tool": attr.label(default = Label(":extract_music_glyphs"), executable = True, cfg = "exec"),
    },
)

def _soundfont_impl(ctx):
    args = ctx.actions.args()
    args.add(ctx.outputs.soundfont)
    args.add_all(ctx.files.samples)
    _run_python(
        ctx,
        args,
        inputs = ctx.files.samples,
        outputs = [ctx.outputs.soundfont],
        mnemonic = "CountInSoundfont",
    )
    ctx.actions.write(ctx.outputs.rust, 'pub static SOUNDFONT: &[u8] = include_bytes!("%s");\n' % ctx.outputs.soundfont.basename)
    return [DefaultInfo(files = depset([ctx.outputs.soundfont, ctx.outputs.rust]))]

count_in_soundfont = rule(
    implementation = _soundfont_impl,
    attrs = {
        "samples": attr.label_list(allow_files = [".pcm"], mandatory = True),
        "soundfont": attr.output(mandatory = True),
        "rust": attr.output(mandatory = True),
        "_tool": attr.label(default = Label(":build_soundfont"), executable = True, cfg = "exec"),
    },
)
