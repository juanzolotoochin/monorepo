"""Render a command script through the real game and encode its frames as MP4."""

load("@rules_python//python:defs.bzl", "py_test")
load(":native_execution.bzl", "NativeExecutionInfo")

GameDemoInfo = provider(fields = ["video", "report", "trace", "audio", "inputs"])

def _demo_impl(ctx):
    if not ctx.attr._native_execution[NativeExecutionInfo].matches_host:
        fail("GPU video demos run locally and require native tools. Build this target without --config=remote.")
    video = ctx.actions.declare_file(ctx.label.name + ".mp4")
    report = ctx.actions.declare_file(ctx.label.name + ".state.json")
    trace = ctx.actions.declare_file(ctx.label.name + ".frames.jsonl")
    audio = ctx.actions.declare_file(ctx.label.name + ".wav")
    inputs_log = ctx.actions.declare_file(ctx.label.name + ".glog")
    inputs_save = ctx.actions.declare_file(ctx.label.name + ".glog.gsav")
    args = ctx.actions.args()
    args.add("--game", ctx.executable.game)
    args.add("--script", ctx.file.script)
    args.add("--ffmpeg", ctx.executable._ffmpeg)
    args.add("--video", video)
    args.add("--report", report)
    args.add("--trace", trace)
    args.add("--audio", audio)
    args.add("--inputs", inputs_log)
    inputs = [ctx.file.script]
    if ctx.file.assertions:
        inputs.append(ctx.file.assertions)
        args.add("--assertions", ctx.file.assertions)
    ctx.actions.run(
        executable = ctx.attr._runner[DefaultInfo].files_to_run,
        arguments = [args],
        inputs = inputs,
        tools = [ctx.attr.game[DefaultInfo].files_to_run, ctx.attr._ffmpeg[DefaultInfo].files_to_run],
        outputs = [video, report, trace, audio, inputs_log, inputs_save],
        mnemonic = "GameDemo",
        progress_message = "Rendering controlled game demo %{label}",
        # GPU drivers belong to the native client host, checked above.
        # Keep hardware-dependent renders off remote workers and caches.
        execution_requirements = {"local": "1", "no-remote": "1", "no-sandbox": "1"},
        use_default_shell_env = True,
    )
    return [
        DefaultInfo(files = depset([video, report, trace, audio, inputs_log, inputs_save])),
        GameDemoInfo(video = video, report = report, trace = trace, audio = audio, inputs = [inputs_log, inputs_save]),
        OutputGroupInfo(video = depset([video]), state = depset([report]), trace = depset([trace]), audio = depset([audio]), inputs = depset([inputs_log, inputs_save])),
    ]

_game_demo = rule(
    implementation = _demo_impl,
    attrs = {
        "game": attr.label(executable = True, cfg = "exec", mandatory = True),
        "assertions": attr.label(allow_single_file = [".json"]),
        "script": attr.label(allow_single_file = [".json"], mandatory = True),
        "_runner": attr.label(default = "//kestrel/tools/demo:runner", executable = True, cfg = "exec"),
        "_ffmpeg": attr.label(default = "@game_demo_tools//:ffmpeg", executable = True, allow_single_file = True, cfg = "exec"),
        "_native_execution": attr.label(default = "//kestrel/build/game_demo:native_execution", cfg = "exec"),
    },
)

def _share_demo_impl(ctx):
    video = ctx.attr.video[GameDemoInfo].video if GameDemoInfo in ctx.attr.video else ctx.attr.video[StudioDemoInfo].video
    publisher = ctx.executable._publisher
    launcher = ctx.actions.declare_file(ctx.label.name + ".sh")
    ctx.actions.write(
        output = launcher,
        content = "\n".join([
            "#!/bin/sh",
            "set -eu",
            'runfiles="${RUNFILES_DIR:-$0.runfiles}"',
            'for root in "$runfiles"/*; do',
            '  if [ -f "$root/%s" ] && [ -x "$root/%s" ]; then' % (video.short_path, publisher.short_path),
            '    exec "$root/%s" --video "$root/%s" "$@"' % (publisher.short_path, video.short_path),
            '  fi',
            'done',
            'echo "Demo video or publisher missing from runfiles" >&2',
            'exit 1',
            "",
        ]),
        is_executable = True,
    )
    runfiles = ctx.runfiles(files = [video, publisher]).merge(ctx.attr._publisher[DefaultInfo].default_runfiles)
    return DefaultInfo(executable = launcher, runfiles = runfiles)

_share_demo = rule(
    implementation = _share_demo_impl,
    executable = True,
    attrs = {
        "video": attr.label(mandatory = True),
        "_publisher": attr.label(default = "//kestrel/tools/telegram:publish", executable = True, cfg = "target"),
    },
)

def game_demo(name, **kwargs):
    """Render a demo and create `<name>_share` to send its MP4 to Telegram."""
    _game_demo(name = name, **kwargs)
    _share_demo(
        name = name + "_share",
        video = ":" + name,
        tags = kwargs.get("tags", []),
        visibility = kwargs.get("visibility"),
    )

StudioDemoInfo = provider(fields = ["video"])

def _studio_demo_impl(ctx):
    if not ctx.attr._native_execution[NativeExecutionInfo].matches_host:
        fail("Studio videos run locally and require native tools. Build without --config=remote.")
    video = ctx.actions.declare_file(ctx.label.name + ".mp4")
    args = ctx.actions.args()
    args.add("--game", ctx.executable.game)
    args.add("--ffmpeg", ctx.executable._ffmpeg)
    args.add("--video", video)
    args.add("--character", ctx.attr.character)
    args.add("--clip", ctx.attr.clip)
    args.add("--angle", ctx.attr.angle)
    args.add("--frames", ctx.attr.frames)
    ctx.actions.run(
        executable = ctx.attr._runner[DefaultInfo].files_to_run,
        arguments = [args],
        tools = [ctx.attr.game[DefaultInfo].files_to_run, ctx.attr._ffmpeg[DefaultInfo].files_to_run],
        outputs = [video],
        mnemonic = "StudioDemo",
        progress_message = "Rendering Character Studio demo %{label}",
        execution_requirements = {"local": "1", "no-remote": "1", "no-sandbox": "1"},
        use_default_shell_env = True,
    )
    return [DefaultInfo(files = depset([video])), StudioDemoInfo(video = video), OutputGroupInfo(video = depset([video]))]

_studio_demo = rule(
    implementation = _studio_demo_impl,
    attrs = {
        "game": attr.label(executable = True, cfg = "exec", mandatory = True),
        "character": attr.string(mandatory = True),
        "clip": attr.string(mandatory = True),
        "angle": attr.string(default = "side"),
        "frames": attr.int(default = 120),
        "_runner": attr.label(default = "//kestrel/tools/demo:studio_runner", executable = True, cfg = "exec"),
        "_ffmpeg": attr.label(default = "@game_demo_tools//:ffmpeg", executable = True, allow_single_file = True, cfg = "exec"),
        "_native_execution": attr.label(default = "//kestrel/build/game_demo:native_execution", cfg = "exec"),
    },
)

def studio_demo(name, game, character, clip, angle = "side", frames = 120, **kwargs):
    """Record a studio clip and create `<name>_share` for Telegram."""
    _studio_demo(
        name = name, game = game, character = character,
        clip = clip, angle = angle, frames = frames, **kwargs
    )
    _share_demo(
        name = name + "_share",
        video = ":" + name,
        tags = kwargs.get("tags", []),
        visibility = kwargs.get("visibility"),
    )

GameSimulationInfo = provider(fields = ["report", "trace", "inputs"])

def _simulation_impl(ctx):
    report = ctx.outputs.report
    trace = ctx.outputs.trace
    ctx.actions.run(
        executable = ctx.attr.game[DefaultInfo].files_to_run,
        arguments = [
            "--simulation",
            "--demo-script",
            ctx.file.script.path,
            "--report",
            report.path,
            "--trace",
            trace.path,
            "--record-input",
            ctx.outputs.inputs.path,
        ],
        inputs = [ctx.file.script],
        tools = [ctx.attr.game[DefaultInfo].files_to_run],
        outputs = [report, trace, ctx.outputs.inputs, ctx.outputs.inputs_save],
        mnemonic = "GameSimulation",
        progress_message = "Recording game simulation %{label}",
    )
    return [
        DefaultInfo(files = depset([report, trace, ctx.outputs.inputs, ctx.outputs.inputs_save])),
        GameSimulationInfo(report = report, trace = trace, inputs = [ctx.outputs.inputs, ctx.outputs.inputs_save]),
        OutputGroupInfo(state = depset([report]), trace = depset([trace]), inputs = depset([ctx.outputs.inputs, ctx.outputs.inputs_save])),
    ]

game_simulation = rule(
    implementation = _simulation_impl,
    outputs = {
        "report": "%{name}.state.json",
        "trace": "%{name}.frames.jsonl",
        # The tick-indexed input log and the save it starts from.
        "inputs": "%{name}.glog",
        "inputs_save": "%{name}.glog.gsav",
    },
    attrs = {
        "game": attr.label(executable = True, cfg = "exec", mandatory = True),
        "script": attr.label(allow_single_file = [".json"], mandatory = True),
    },
)

def game_simulation_test(name, game, script, assertions, **kwargs):
    """Build a deterministic state dump, then assert its recorded world states."""
    capture = name + "_capture"
    game_simulation(name = capture, game = game, script = script)
    py_test(
        name = name,
        srcs = ["//kestrel/tools/demo:simulation_test.py"],
        main = "//kestrel/tools/demo:simulation_test.py",
        deps = ["//kestrel/tools/demo:trace_assertions"],
        data = [":" + capture + ".state.json", ":" + capture + ".frames.jsonl", assertions],
        args = [
            "--report",
            "$(rootpath :%s.state.json)" % capture,
            "--trace",
            "$(rootpath :%s.frames.jsonl)" % capture,
            "--assertions",
            "$(rootpath %s)" % assertions,
        ],
        **kwargs
    )
