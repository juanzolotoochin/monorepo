"""Hermetic, renderer-independent voxel prefab compilation and scene packing."""

load("@rules_rs//rs:rust_library.bzl", "rust_library")

VoxelPrefabInfo = provider(fields = {"asset_id": "Unique prefab ID", "file": "Compiled prefab artifact"})
ScenePackInfo = provider(fields = {"file": "Validated scene pack", "rust": "Embedding module"})

def _prefab_impl(ctx):
    args = ctx.actions.args()
    args.add("prefab")
    args.add("--recipe", ctx.file.src)
    inputs = [ctx.file.src]
    if ctx.file.style and ctx.file.palette:
        fail("Choose either a palette or a style for a prefab")
    if ctx.file.style:
        inputs.append(ctx.file.style)
        args.add("--style", ctx.file.style)
    elif ctx.file.palette:
        inputs.append(ctx.file.palette)
        args.add("--palette", ctx.file.palette)
    else:
        fail("A voxel prefab requires a palette or style")
    args.add("--id", ctx.label.name)
    args.add("--mode", ctx.attr.mode)
    args.add("--out", ctx.outputs.pack)
    ctx.actions.run(
        executable = ctx.attr._compiler[DefaultInfo].files_to_run,
        arguments = [args],
        inputs = inputs,
        outputs = [ctx.outputs.pack],
        mnemonic = "VoxelPrefab",
        progress_message = "Compiling voxel prefab %{label}",
    )
    return [
        DefaultInfo(files = depset([ctx.outputs.pack])),
        VoxelPrefabInfo(asset_id = ctx.label.name, file = ctx.outputs.pack),
    ]

voxel_prefab = rule(
    implementation = _prefab_impl,
    attrs = {
        "src": attr.label(allow_single_file = [".ron"], mandatory = True),
        "palette": attr.label(allow_single_file = [".ron"]),
        "style": attr.label(allow_single_file = [".ron"]),
        "mode": attr.string(default = "baked", values = ["baked", "runtime"]),
        "_compiler": attr.label(default = "//tools/assetc:assetc", executable = True, cfg = "exec"),
    },
    outputs = {"pack": "%{name}.prefab.json"},
)

def _scene_impl(ctx):
    args = ctx.actions.args()
    args.add("scene")
    args.add("--layout", ctx.file.layout)
    args.add("--out", ctx.outputs.pack)
    args.add("--rust-out", ctx.outputs.rust)
    seen = {}
    inputs = [ctx.file.layout]
    if ctx.file.style:
        inputs.append(ctx.file.style)
        args.add("--style", ctx.file.style)
    for dep in ctx.attr.prefabs:
        prefab = dep[VoxelPrefabInfo]
        if prefab.asset_id in seen:
            fail("Duplicate prefab ID: " + prefab.asset_id)
        seen[prefab.asset_id] = True
        inputs.append(prefab.file)
        args.add("--prefab", prefab.file)
    ctx.actions.run(
        executable = ctx.attr._compiler[DefaultInfo].files_to_run,
        arguments = [args],
        inputs = inputs,
        outputs = [ctx.outputs.pack, ctx.outputs.rust],
        mnemonic = "VoxelScene",
        progress_message = "Packing voxel scene %{label}",
    )
    return [
        DefaultInfo(files = depset([ctx.outputs.pack])),
        ScenePackInfo(file = ctx.outputs.pack, rust = ctx.outputs.rust),
    ]

_scene_pack = rule(
    implementation = _scene_impl,
    attrs = {
        "style": attr.label(allow_single_file = [".ron"]),
        "layout": attr.label(allow_single_file = [".ron"], mandatory = True),
        "prefabs": attr.label_list(providers = [VoxelPrefabInfo], mandatory = True),
        "_compiler": attr.label(default = "//tools/assetc:assetc", executable = True, cfg = "exec"),
    },
    outputs = {"pack": "%{name}.scene.json", "rust": "%{name}.rs"},
)

def boulder_prefab(name, src, palette = None, runtime = False, **kwargs):
    """Convenience preset; runtime mode stores the recipe instead of baking it."""
    voxel_prefab(name = name, src = src, palette = palette, mode = "runtime" if runtime else "baked", **kwargs)

def scene_assets(name, layout, prefabs, style = None, **kwargs):
    """Validate a scene and expose its embedded bytes as a Rust library target."""
    pack = name + "_pack"
    _scene_pack(name = pack, layout = layout, prefabs = prefabs, style = style)
    rust_library(
        name = name,
        srcs = [":" + pack + ".rs"],
        compile_data = [":" + pack],
        edition = "2021",
        **kwargs
    )
