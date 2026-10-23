"""Render a command script through the real game and encode its frames as MP4."""

load("@rules_python//python:defs.bzl", "py_test")

GameDemoInfo = provider(fields = ["video", "report", "trace", "audio"])

def _demo_impl(ctx):
    video = ctx.actions.declare_file(ctx.label.name + ".mp4")
    report = ctx.actions.declare_file(ctx.label.name + ".state.json")
    trace = ctx.actions.declare_file(ctx.label.name + ".frames.jsonl")
    audio = ctx.actions.declare_file(ctx.label.name + ".wav")
    args = ctx.actions.args()
    args.add("--game", ctx.executable.game)
    args.add("--script", ctx.file.script)
    args.add("--ffmpeg", ctx.executable._ffmpeg)
    args.add("--video", video)
    args.add("--report", report)
    args.add("--trace", trace)
    args.add("--audio", audio)
    inputs = [ctx.file.script]
    if ctx.file.assertions:
        inputs.append(ctx.file.assertions)
        args.add("--assertions", ctx.file.assertions)
    ctx.actions.run(
        executable = ctx.attr._runner[DefaultInfo].files_to_run,
        arguments = [args],
        inputs = inputs,
        tools = [ctx.attr.game[DefaultInfo].files_to_run, ctx.attr._ffmpeg[DefaultInfo].files_to_run],
        outputs = [video, report, trace, audio],
        mnemonic = "GameDemo",
        progress_message = "Rendering controlled game demo %{label}",
        # GPU drivers and the configured encoder belong to this execution host.
        # Keep hardware-dependent renders off remote workers and caches.
        execution_requirements = {"local": "1", "no-remote": "1", "no-sandbox": "1"},
        use_default_shell_env = True,
    )
    return [
        DefaultInfo(files = depset([video, report, trace, audio])),
        GameDemoInfo(video = video, report = report, trace = trace, audio = audio),
        OutputGroupInfo(video = depset([video]), state = depset([report]), trace = depset([trace]), audio = depset([audio])),
    ]

game_demo = rule(
    implementation = _demo_impl,
    attrs = {
        "game": attr.label(executable = True, cfg = "exec", mandatory = True),
        "assertions": attr.label(allow_single_file = [".json"]),
        "script": attr.label(allow_single_file = [".json"], mandatory = True),
        "_runner": attr.label(default = "//tools/game_demo:runner", executable = True, cfg = "exec"),
        "_ffmpeg": attr.label(default = "@game_demo_tools//:ffmpeg", executable = True, allow_single_file = True, cfg = "exec"),
    },
)

GameSimulationInfo = provider(fields = ["report", "trace"])

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
        ],
        inputs = [ctx.file.script],
        tools = [ctx.attr.game[DefaultInfo].files_to_run],
        outputs = [report, trace],
        mnemonic = "GameSimulation",
        progress_message = "Recording game simulation %{label}",
    )
    return [
        DefaultInfo(files = depset([report, trace])),
        GameSimulationInfo(report = report, trace = trace),
        OutputGroupInfo(state = depset([report]), trace = depset([trace])),
    ]

game_simulation = rule(
    implementation = _simulation_impl,
    outputs = {"report": "%{name}.state.json", "trace": "%{name}.frames.jsonl"},
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
        srcs = ["//tools/game_demo:simulation_test.py"],
        main = "//tools/game_demo:simulation_test.py",
        deps = ["//tools/game_demo:trace_assertions"],
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
