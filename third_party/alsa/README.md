# ALSA

`//third_party/alsa` aliases `@alsa_lib//:asound` from the
[Bazel Central Registry](https://registry.bazel.build/modules/alsa_lib).
The version is pinned in `MODULE.bazel` and the registry metadata is recorded
in `MODULE.bazel.lock`.

The registry's `1.2.9.bcr.4` module builds ALSA 1.2.9 with native `cc_library`
rules and generated headers. It replaces the unused local ALSA 1.2.12
`configure_make` overlay, whose configure probes fail with the repository's
hermetic LLVM toolchain. The registry version is older, but provides the
sequencer API used by MIDI Keys and avoids host configure/make detection.
The local `alsa_lib_hermetic_headers.patch` enables Bazel symlink actions for
header staging, replacing the registry overlay's shell calls to host `cp`.

```sh
bazel build --config=full_local //third_party/alsa
# Verify build actions do not resolve tools through the host PATH.
bazel build --config=full_local --action_env=PATH=/nonexistent //salsa/tools/midi_keys
```

This is a Linux dependency. ALSA is linked statically into MIDI Keys; the
application supplies its own sequencer configuration. Kernel MIDI devices
and permissions are runtime requirements, not build dependencies.
