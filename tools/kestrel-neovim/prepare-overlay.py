"""Faithful game-source snapshot for saved-file validation; never edits kestrel/."""
import json
from pathlib import Path
import shutil
import tomllib

root = Path(__file__).resolve().parents[2]
cache = root / '.rules_rust_analyzer/cargo-ide'
overlay = cache / 'validation-overlay'
workspace = overlay / 'workspace'
source = overlay / 'source'
assert overlay.resolve().is_relative_to(cache.resolve())
shutil.copytree(root / 'kestrel', source / 'kestrel', dirs_exist_ok=True,
                ignore=shutil.ignore_patterns('BUILD', 'BUILD.bazel', '*.bzl'))
# Earlier snapshots included Bazel definitions. They are not compiler inputs
# and must not introduce duplicate packages into recursive Bazel discovery.
for path in (source / 'kestrel').rglob('*'):
    if path.is_file() and (path.name in ('BUILD', 'BUILD.bazel') or path.suffix == '.bzl'):
        assert path.resolve().is_relative_to((source / 'kestrel').resolve())
        path.unlink()
# Inject only into the owned overlay; the real source file stays byte-identical.
flow = source / 'kestrel/app/src/flow.rs'
text = flow.read_text(encoding='utf-8')
assert 'self.opened.take()' in text
text = text.replace('self.opened.take()', 'let __kestrel_syntax_probe = ; self.opened.take()', 1)
flow.write_text(text, encoding='utf-8', newline='\n')

data = tomllib.loads((cache / 'Cargo.toml').read_text(encoding='utf-8'))
paths = [cache / 'Cargo.toml']
for member in data['workspace']['members']:
    paths.extend(p for p in (cache / member).rglob('*') if p.is_file())
for path in paths:
    target = workspace / path.relative_to(cache)
    target.parent.mkdir(parents=True, exist_ok=True)
    text = path.read_text(encoding='utf-8')
    text = text.replace(cache.as_posix(), workspace.as_posix())
    text = text.replace((root / 'kestrel').as_posix(), (source / 'kestrel').as_posix())
    target.write_text(text, encoding='utf-8', newline='\n')
shutil.copyfile(cache / 'Cargo.lock', workspace / 'Cargo.lock')
print(json.dumps({'workspace': workspace.as_posix(), 'source': source.as_posix(),
                  'artifacts': 'Reuse external dependency artifacts in cargo-ide/target; new first-party package paths'}))
