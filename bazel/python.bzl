load("@aspect_rules_lint//lint:ruff.bzl", "lint_ruff_aspect")
load("@pip_deps//:requirements.bzl", "requirement")
load("@rules_python//python:defs.bzl", "PyInfo", _py_binary = "py_binary", _py_library = "py_library", _py_test = "py_test")

ruff_aspect = lint_ruff_aspect(
    binary = Label("@aspect_rules_lint//lint:ruff_bin"),
    configs = [Label("//:pyproject.toml")],
)

FIRECRACKER_EXEC_PROPERTIES = {
    # Tell BuildBuddy to run this test using a Firecracker microVM.
    "test.workload-isolation-type": "firecracker",
    # Tell BuildBuddy to ensure that the Docker daemon is started
    # inside the microVM before the test starts, so that we don't
    # have to worry about starting it ourselves.
    "test.init-dockerd": "true",
    # Tell BuildBuddy to preserve the microVM state across test runs.
    "test.recycle-runner": "true",
    "container-image": "docker://docker.io/juanzolotoochin/ubuntu-build-v2@sha256:4a898ae754ac575962392232dc0154937427bbd52f5b79cd65c0992b2ed6cc84",
}

def py_binary(name, srcs, **kwargs):
    """ Wrapper for py_binary rule that adds additional functionality.

    Two additional targets are added, one for .par target and one for mypy check
    test.

    Args:
     name: Name that will be used for the native py_binary target.
     **kwargs: All other target args.
    """

    if "main" not in kwargs:
        if len(srcs) == 1:
            kwargs["main"] = srcs[0]
        else:
            fail("Missing main attribute for multi srcs target.")

    _py_binary(name = name, srcs = srcs, **kwargs)

def py_library(name, srcs, **kwargs):
    _py_library(name = name, srcs = srcs, **kwargs)

def py_test(name, srcs, firecracker = False, **kwargs):
    if firecracker:
        _py_test(
            name = name,
            exec_properties = FIRECRACKER_EXEC_PROPERTIES,
            srcs = srcs,
            **kwargs
        )
    else:
        _py_test(
            name = name,
            srcs = srcs,
            **kwargs
        )

    py_debug(name = name + "_debug", srcs = srcs, og_name = name, **kwargs)

def pylint_test(name, srcs, deps = [], args = [], data = [], **kwargs):
    kwargs["main"] = "pylint_test_wrapper.py"
    _py_test(
        name = name,
        srcs = ["//bazel/workspace/tools/pylint:pylint_test_wrapper.py"] + srcs,
        args = ["--pylint-rcfile=$(location //bazel/workspace/tools/pylint:.pylintrc)"] + args + ["$(location :%s)" % x for x in srcs],
        deps = deps + [
            "@pip_deps//pytest",
            "@pip_deps//pytest-pylint",
        ],
        data = [
            "//bazel/workspace/tools/pylint:.pylintrc",
        ] + data,
        **kwargs
    )

def py_debug(name, og_name, srcs, deps = [], **kwargs):
    wrapper_dep_name = og_name + "_debug_wrapper"
    wrapper_filename = wrapper_dep_name + ".py"

    py_debug_wrapper(name = wrapper_dep_name, out = wrapper_filename)

    _py_binary(
        name = name,
        srcs = [wrapper_filename] + srcs,
        main = wrapper_filename,
        deps = deps + [
            requirement("pytest"),
            requirement("debugpy"),
            ":" + wrapper_dep_name,
        ],
        **kwargs
    )

def _py_debug_wrapper_impl(ctx):
    path = str(ctx.label)
    full_module = path.replace("//", "").replace("/", ".").replace(":", ".").replace("_debug_wrapper", "").replace("@", "")

    ctx.actions.expand_template(
        template = ctx.file._template,
        output = ctx.outputs.out,
        substitutions = {
            "{FULL_MODULE}": full_module,
        },
    )
    return [PyInfo(transitive_sources = depset([ctx.outputs.out]))]

py_debug_wrapper = rule(
    implementation = _py_debug_wrapper_impl,
    attrs = {
        "_template": attr.label(
            default = Label("//bazel/workspace/tools/pydebug:pydebug_wrapper.py.template"),
            allow_single_file = True,
        ),
        "out": attr.output(mandatory = True),
    },
)

def pytest_test(name, srcs, deps = [], args = [], **kwargs):
    _py_test(
        name = name,
        srcs = [
            "//bazel/workspace/tools/pytest:pytest_wrapper.py",
        ] + srcs,
        main = "//bazel/workspace/tools/pytest:pytest_wrapper.py",
        args = [
            "--capture=no",
        ] + args + ["$(location :%s)" % x for x in srcs],
        deps = deps + [
            requirement("pytest"),
        ],
        **kwargs
    )

def py_executable(name, binary):
    zip_target_name = binary.replace(":", "") + "_zip"
    native.filegroup(
        name = zip_target_name,
        srcs = [binary],
        output_group = "python_zip_file",
    )

    _py_executable_wrapper(name = name, binary = zip_target_name)

def _py_executable_wrapper_impl(ctx):
    output = ctx.actions.declare_file(ctx.attr.name)
    input = ctx.file.binary
    ctx.actions.run_shell(
        inputs = [input],
        outputs = [output],
        arguments = [input.path, output.path],
        command = "echo '#!/usr/bin/env python' >> $2 && cat $1 >> $2",
    )

    return [DefaultInfo(files = depset([output]))]

_py_executable_wrapper = rule(
    implementation = _py_executable_wrapper_impl,
    attrs = {
        "binary": attr.label(allow_single_file = True, mandatory = True),
    },
)
