## Monorepo

### Getting started

```
# Clone the repository
git clone https://github.com/juanique/monorepo.git
cd monorepo

# Run a simple Python binary
bazel run //examples/python:version
```

For a small Rust game scene, run the [Kestrel voxel sci-fi scene](game/app/README.md):

```sh
bazel run //game/app:demo
```
