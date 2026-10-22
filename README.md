## Monorepo

### Getting started

```
# Clone the repository
git clone https://github.com/juanique/monorepo.git
cd monorepo

# Run a simple Python binary
bazel run //examples/python:version
```

For a small Rust game scene, run the [Bevy isometric garden](examples/rust/bevy_isometric/README.md):

```sh
bazel run //examples/rust/bevy_isometric:demo
```
