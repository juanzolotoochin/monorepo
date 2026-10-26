"""Personal, opt-in Kestrel Cargo editor cache. Normal Bazel builds never import this.

Python 3.11+; run `python tools/kestrel-neovim/cache.py prepare` from the checkout.
No source files or global editor/toolchain settings are written.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import sys
import time
import tomllib

ROOT = Path(__file__).resolve().parents[2]
CACHE = ROOT / '.rules_rust_analyzer/cargo-ide'
TOOLS = Path(__file__).resolve().parent
VERSION = '1.97.1'
# Match `bazel run --config=remote`: remote execution, native Windows editor target.
# The target override is required for Cargo's local Windows proc-macro/check path.
# Docker remains first; Windows is needed only to resolve native test toolchains.
BAZEL_NATIVE = ['--config=remote', '--platforms=@native_platform//:host',
                '--extra_execution_platforms=//:docker_image_platform,//platforms:rust_windows_x86_64']


def write(path: Path, text: str):
    path.parent.mkdir(parents=True, exist_ok=True)
    if not path.exists() or path.read_text(encoding='utf-8') != text:
        path.write_text(text, encoding='utf-8', newline='\n')


def j(value):
    return json.dumps(value, ensure_ascii=False)


def run(args, output=None, cwd=ROOT, env=None, allow_failure=False):
    started = time.monotonic()
    print('+', subprocess.list2cmdline([str(a) for a in args]), flush=True)
    if output:
        output.parent.mkdir(parents=True, exist_ok=True)
        with output.open('w', encoding='utf-8') as stream:
            result = subprocess.run(args, cwd=cwd, env=env, stdout=stream,
                                    stderr=subprocess.PIPE, text=True)
    else:
        result = subprocess.run(args, cwd=cwd, env=env, stderr=subprocess.PIPE, text=True)
    if result.stderr:
        lines = result.stderr.splitlines()
        print('\n'.join(lines[-35:]), file=sys.stderr, flush=True)
    with (CACHE / 'runs.jsonl').open('a', encoding='utf-8') as stream:
        stream.write(json.dumps({'args': [str(a) for a in args], 'seconds': time.monotonic() - started,
                                 'returncode': result.returncode, 'stderr': result.stderr}) + '\n')
    # cquery can exit zero despite export errors.
    if not allow_failure and (result.returncode or 'ERROR:' in (result.stderr or '')):
        raise RuntimeError(f'Command failed ({result.returncode}); cache not ready')
    return round(time.monotonic() - started, 3)


def canonical(label):
    return '@@' + label if label.startswith('//') else label


def input_files():
    paths = [ROOT / n for n in ['Cargo.toml', 'Cargo.lock', 'MODULE.bazel',
                                'MODULE.bazel.lock', '.bazelrc', '.bazelversion',
                                '.rules_rust_analyzer/editor-settings.json']]
    paths += sorted((ROOT / 'kestrel').rglob('BUILD.bazel'))
    paths += sorted((ROOT / 'kestrel').rglob('*.bzl'))
    paths += sorted((ROOT / 'bazel/patches').glob('*.patch'))
    paths += [TOOLS / name for name in ('cache.py', 'export.cquery', 'rustc-wrapper.rs')]
    # Asset recipes affect generated sources/data; ordinary Rust edits do not
    # invalidate the project description.
    paths += sorted((ROOT / 'kestrel/app/assets').rglob('*.ron'))
    paths += sorted((ROOT / 'kestrel/tools/assetc').rglob('*.rs'))
    paths.append(ROOT / 'kestrel/app/src/sound.c')
    return [p for p in paths if p.is_file()]


def fingerprint(paths):
    return {p.as_posix(): hashlib.sha256(p.read_bytes()).hexdigest() for p in paths}


def toolchain():
    rustup = Path(os.environ['USERPROFILE']) / '.rustup/toolchains' / (VERSION + '-x86_64-pc-windows-msvc')
    cargo, rustc = rustup / 'bin/cargo.exe', rustup / 'bin/rustc.exe'
    if not cargo.is_file() or not rustc.is_file():
        raise RuntimeError(f'Install rustup toolchain {VERSION}; global default is not changed')
    return cargo, rustc


def cargo_env():
    cargo, rustc = toolchain()
    return dict(os.environ, RUSTUP_TOOLCHAIN=VERSION, CARGO=str(cargo), RUSTC=str(rustc),
                CARGO_TARGET_DIR=str(CACHE / 'target'), RUSTC_WRAPPER=str(CACHE / 'rustc-wrapper.exe'),
                PATH=str(cargo.parent) + os.pathsep + os.environ['PATH'])


def prepare(args):
    CACHE.mkdir(parents=True, exist_ok=True)
    timing = {}
    scope = '//kestrel/...' if args.all_game else '//kestrel/app:demo'
    roots = 'kind("rust_(library|binary|proc_macro|test)", //kestrel/...)' if args.all_game else scope
    if not args.from_cache:
        if args.all_game:
            # cquery analyzes its target universe before filtering by kind.
            # Enumerate Rust labels first so manual GPU/video targets are never analyzed.
            timing['root_enumeration_s'] = run(['bazel', 'query', roots, '--output=label'], CACHE / 'roots.txt')
            labels = (CACHE / 'roots.txt').read_text().split()
            roots = 'set(' + ' '.join(labels) + ')'
        timing['metadata_export_s'] = run([
            'bazel', 'cquery', *BAZEL_NATIVE, f'deps({roots})', '--output=starlark',
            '--starlark:file=tools/kestrel-neovim/export.cquery'], CACHE / 'configured-rust.jsonl')
        timing['attribute_export_s'] = run([
            'bazel', 'cquery', *BAZEL_NATIVE, f'kind("rust_(library|binary|proc_macro|test)", deps({roots}))',
            '--output=jsonproto'], CACHE / 'attributes.json')
        timing['native_export_s'] = run(['bazel', 'cquery', *BAZEL_NATIVE, '//kestrel/app:sound', '--output=files'], CACHE / 'native.txt')
        timing['bazel_info_s'] = run(['bazel', 'info', 'execution_root'], CACHE / 'execution-root.txt')
        timing['asset_native_preparation_s'] = run([
            'bazel', 'build', *BAZEL_NATIVE, '//kestrel/app/assets:all', '//kestrel/app:sound',
            '--remote_download_outputs=toplevel',
            '--remote_download_regex=.*kestrel/app/assets/.*\\.(rs|json)$'])
        write(CACHE / 'last-preparation.json', json.dumps({'scope': scope, 'timing': timing}, indent=2) + '\n')
    elif (CACHE / 'last-preparation.json').exists():
        previous = json.loads((CACHE / 'last-preparation.json').read_text())
        if previous['scope'] == scope:
            timing = previous['timing']
    rows = [json.loads(l) for l in (CACHE / 'configured-rust.jsonl').read_text(encoding='utf-8-sig').splitlines() if l.strip()]
    attrs = {}
    for result in json.loads((CACHE / 'attributes.json').read_text(encoding='utf-8-sig'))['results']:
        rule = result['target'].get('rule', {})
        if not rule:
            continue
        label = canonical(rule['name'])
        a = {v['name']: v for v in rule['attribute']}
        # Features may differ between host/target configurations. Keep all
        # variants for the audit; first-party manifests don't merge cfgs.
        attrs.setdefault(label, []).append(a)
    candidates = {}
    for row in rows:
        if row['label'] != row['owner']:
            continue  # Export aliases point at their actual provider owner.
        candidates.setdefault(row['label'], []).append(row)
    crates = {}
    for label, variants in candidates.items():
        # Select normal target configuration for generated game crates, rather
        # than the asset compiler's host configuration with the same label.
        crates[label] = min(variants, key=lambda c: (c['env'].get('CARGO_CFG_TARGET_OS') != 'windows', '-exec' in c['root'], '-ST-' in c['root'], c['is_test'], c['root']))
    selected = {}

    def visit(label):
        if label in selected:
            return
        if label not in crates:
            raise RuntimeError(f'Missing Rust dependency provider: {label}')
        c = selected[label] = crates[label]
        for dep in c['deps'] + c['proc_macro_deps']:
            visit(dep)

    visit('@@//kestrel/app:demo')
    test_contexts = {}
    if args.all_game:
        ordinary = {c['root']: label for label, c in crates.items() if label.startswith('@@//kestrel/') and not c['is_test']}
        for label, c in sorted(crates.items()):
            if not label.startswith('@@//kestrel/'):
                continue
            if not c['is_test'] or c['root'] not in ordinary:
                visit(label)
            else:
                base = ordinary[c['root']]
                test_contexts.setdefault(base, []).append(c)
                for dep in c['deps'] + c['proc_macro_deps']:
                    visit(dep)
    execution_root = Path((CACHE / 'execution-root.txt').read_text().strip())
    first = {label: c for label, c in selected.items() if label.startswith('@@//kestrel/')}
    external = {label: c for label, c in selected.items() if label not in first}
    packages = {}
    for label, c in external.items():
        source = (execution_root.parent.parent if c['root'].startswith('external/') else execution_root) / c['root']
        if not source.is_file():
            raise RuntimeError(f'External Rust source missing: {source}')
        # Bazel roots can be lib.rs below several nested source directories.
        manifest = next((p / 'Cargo.toml' for p in source.parents if (p / 'Cargo.toml').is_file()), None)
        if manifest is None:
            raise RuntimeError(f'Cannot locate registry manifest: {label}')
        data = tomllib.loads(manifest.read_text(encoding='utf-8'))
        packages[label] = (data['package']['name'], data['package']['version'], manifest.parent, data)
    # gpu-allocator's broad Windows constraint can resolve to windows 0.57
    # in Cargo even though Bazel selected 0.62. Pin this cache-only manifest
    # to the evaluated edge; retain the actual crate source and version.
    for label, (name, version, folder, data) in list(packages.items()):
        if name != 'gpu-allocator':
            continue
        windows = [packages[d][1] for d in selected[label]['deps']
                   if d in packages and packages[d][0] == 'windows']
        if not windows:
            continue
        projected = CACHE / 'dependency-overrides' / ('gpu-allocator-' + version)
        shutil.copytree(folder, projected, dirs_exist_ok=True,
                        ignore=shutil.ignore_patterns('BUILD', 'BUILD.bazel', '*.bzl'))
        manifest = (folder / 'Cargo.toml').read_text(encoding='utf-8')
        section = '[target."cfg(windows)".dependencies.windows]'
        start = manifest.index(section) + len(section)
        end = manifest.find('\n[', start)
        if end < 0:
            end = len(manifest)
        body = re.sub(r'version = "[^"]+"', 'version = "=' + windows[0] + '"', manifest[start:end], count=1)
        write(projected / 'Cargo.toml', manifest[:start] + body + manifest[end:])
        packages[label] = (name, version, projected, tomllib.loads((projected / 'Cargo.toml').read_text(encoding='utf-8')))
    counts = {c['name']: sum(d['name'] == c['name'] for d in first.values()) for c in first.values()}
    package_names = {label: c['name'] if counts[c['name']] == 1 else 'kestrel-' + label[4:].replace('/', '-').replace(':', '-') for label, c in first.items()}
    members = {label: 'crates/' + package_names[label] for label in first}
    if len(set(members.values())) != len(members):
        raise RuntimeError('Kestrel target package names collide; explicit mapping required')
    audit = {'scope': scope, 'first_party_crates': len(first),
             'external_crates': len(external), 'feature_variants': {}, 'native': {},
             'env_translation': {}, 'timing': timing}
    audit['packages'] = {label: {'name': name, 'version': version, 'manifest': (folder / 'Cargo.toml').as_posix()} for label, (name, version, folder, _) in packages.items()}
    audit['game_targets'] = {label: {'package': package_names[label], 'root': c['root'], 'test_only': c['is_test']} for label, c in first.items()}
    audit['unit_test_targets'] = {label: [c['label'] for c in contexts] for label, contexts in test_contexts.items()}
    # Cargo's feature resolution otherwise drops features unified by Bazel's
    # monorepo-wide resolver. Small support crates activate the evaluated
    # feature sets without changing application sources or copying dependencies.
    def split_features(label, data):
        wanted = sorted({f for a in attrs.get(label, []) for f in a.get('crate_features', {}).get('stringListValue', [])})
        declared = set(data.get('features', {}))
        suppressed = {v[4:] for values in data.get('features', {}).values() for v in values if v.startswith('dep:')}
        for name, spec in data.get('dependencies', {}).items():
            if isinstance(spec, dict) and spec.get('optional') and name not in suppressed:
                declared.add(name)
        return [f for f in wanted if f in declared], [f for f in wanted if f not in declared]

    helper_members = []
    policy = []
    for n, (label, (name, version, folder, data)) in enumerate(sorted(packages.items())):
        index = n // 24
        member = f'features/batch-{index}'
        if member not in helper_members:
            helper_members.append(member)
        valid, synthetic = split_features(label, data)
        policy.append('\t'.join([folder.as_posix().lower(), '--cap-lints=allow'] + ['--cfg=feature=' + j(f) for f in synthetic]))
    for index, member in enumerate(helper_members):
        lines = ['[package]', f'name = "kestrel-editor-features-{index}"', 'version = "0.0.0"',
                 'edition = "2021"', 'publish = false', '[dependencies]']
        for n, (label, (name, version, folder, data)) in enumerate(sorted(packages.items())):
            if n // 24 != index:
                continue
            valid, _ = split_features(label, data)
            alias = 'dep_' + str(n)
            lines.append(f'{alias} = {{ package = {j(name)}, version = {j("=" + version)}, default-features = false, features = {j(valid)} }}')
        write(CACHE / member / 'Cargo.toml', '\n'.join(lines) + '\n')
        write(CACHE / member / 'src/lib.rs', '// Cargo feature activation only; real dependency sources remain unchanged.\n')
    write(CACHE / 'rustc-policy.tsv', '\n'.join(policy) + '\n')
    cargo, rustc = toolchain()
    wrapper = CACHE / 'rustc-wrapper.exe'
    if not wrapper.exists() or wrapper.stat().st_mtime < (TOOLS / 'rustc-wrapper.rs').stat().st_mtime:
        run([str(rustc), str(TOOLS / 'rustc-wrapper.rs'), '-O', '-o', str(wrapper)])
    audit['feature_support_crates'] = len(helper_members)
    root_lines = ['# Generated personal Kestrel editor workspace. Bazel remains authoritative.',
                  '[workspace]', 'resolver = "2"', 'members = ' + j(sorted(list(members.values()) + helper_members)),
                  'default-members = ["crates/demo"]', '', '[patch.crates-io]']
    # Include build-time crates too: cc/rav1e/khronos_api may be patched by
    # Bazel even when absent from the Rust application's direct analysis graph.
    patches = {(name, version): folder for name, version, folder, _ in packages.values()}
    for label, c in crates.items():
        if label.startswith('@@//'):
            continue
        source = (execution_root.parent.parent if c['root'].startswith('external/') else execution_root) / c['root']
        manifest = next((p / 'Cargo.toml' for p in source.parents if (p / 'Cargo.toml').is_file()), None)
        if manifest:
            data = tomllib.loads(manifest.read_text(encoding='utf-8'))['package']
            patches.setdefault((data['name'], data['version']), manifest.parent)
    for (name, version), folder in sorted(patches.items()):
        if name == 'tinyjson' and not any(p[0] == name for p in packages.values()):
            continue  # Bazel helper tooling; not part of Kestrel's Cargo graph.
        key = name + '-' + version.replace('.', '_').replace('+', '_')
        root_lines.append(f'{j(key)} = {{ package = {j(name)}, path = {j(folder.as_posix())} }}')
    write(CACHE / 'Cargo.toml', '\n'.join(root_lines) + '\n')
    if not (CACHE / 'Cargo.lock').exists():
        shutil.copyfile(ROOT / 'Cargo.lock', CACHE / 'Cargo.lock')
    generated = []
    for label, c in first.items():
        folder = CACHE / members[label]
        source = (execution_root if c['root'].startswith('bazel-out/') else ROOT) / c['root']
        if not source.is_file():
            raise RuntimeError(f'Generated source absent; rerun prepare: {source}')
        if c['root'].startswith('bazel-out/'):
            generated.extend([source] + [execution_root / f for f in c['compile_data']])
        a = attrs[label][0]
        lines = ['[package]', f'name = {j(package_names[label])}', 'version = "0.0.0"',
                 f'edition = {j(c["edition"])}', 'publish = false', 'autobins = false',
                 'autoexamples = false', 'autotests = false', 'autobenches = false', 'build = "build.rs"',
                 '', '[[test]]' if c['is_test'] else '[[bin]]' if c['type'] == 'bin' else '[lib]',
                 f'name = {j(c["name"])}', f'path = {j(source.as_posix())}', '', '[dependencies]']
        for dep in c['deps'] + c['proc_macro_deps']:
            dc = selected[dep]
            alias = c['aliases'].get(dep, dc['name'])
            if dep in first:
                spec = f'package = {j(package_names[dep])}, path = {j((CACHE / members[dep]).as_posix())}'
            else:
                name, version, _, manifest_data = packages[dep]
                variants = attrs[dep]
                features, _ = split_features(dep, manifest_data)
                audit['feature_variants'][dep] = sorted({tuple(v.get('crate_features', {}).get('stringListValue', [])) for v in variants})
                spec = f'package = {j(name)}, version = {j("=" + version)}, default-features = false, features = {j(features)}'
            lines.append(f'{j(alias)} = {{ {spec} }}')
        if label == '@@//kestrel/app:demo':
            for index, member in enumerate(helper_members):
                lines.append(f'kestrel-editor-features-{index} = {{ path = {j((CACHE / member).as_posix())} }}')
        extra = {dep for context in test_contexts.get(label, []) for dep in context['deps'] + context['proc_macro_deps']} - set(c['deps'] + c['proc_macro_deps'])
        if extra:
            lines += ['', '[dev-dependencies]']
            for dep in sorted(extra):
                alias = selected[dep]['name']
                if dep in first:
                    spec = f'package = {j(package_names[dep])}, path = {j((CACHE / members[dep]).as_posix())}'
                else:
                    name, version, _, data = packages[dep]
                    features, _ = split_features(dep, data)
                    spec = f'package = {j(name)}, version = {j("=" + version)}, default-features = false, features = {j(features)}'
                lines.append(f'{j(alias)} = {{ {spec} }}')
        build = ['fn main() {']
        # Cargo owns CARGO_* metadata and target variables in this projection.
        # Preserve non-Cargo environment settings; reject unknown substitution.
        translated = {}
        for key, value in c['env'].items():
            if key.startswith('CARGO_') or key in ('LIB', 'REPOSITORY_NAME'):
                continue
            value = value.replace('${pwd}', ROOT.as_posix())
            if '${' in value:
                raise RuntimeError(f'Untranslated environment variable {label}: {key}={value}')
            translated[key] = value
            build.append('    println!("{}", ' + j('cargo:rustc-env=' + key + '=' + value) + ');')
        audit['env_translation'][label] = {'preserved': translated, 'cargo_metadata': 'owned by generated Cargo package'}
        flags = a.get('rustc_flags', {}).get('stringListValue', [])
        if flags:
            raise RuntimeError(f'First-party rustc_flags need explicit translation: {label}: {flags}')
        link_deps = a.get('link_deps', {}).get('stringListValue', [])
        if link_deps:
            if link_deps != ['//kestrel/app:sound']:
                raise RuntimeError(f'Unsupported native dependencies: {link_deps}')
            native = [(execution_root / p.strip()) for p in (CACHE / 'native.txt').read_text().splitlines() if p.strip()]
            libs = sorted({p for p in native if p.suffix == '.lib'}, key=lambda p: ('-exec' in p.as_posix(), '-ST-' in p.as_posix(), p.as_posix()))[:1]
            if len(libs) != 1 or not libs[0].exists():
                raise RuntimeError(f'Expected one prepared native sound library: {libs}')
            lib = libs[0]
            generated.append(lib)
            build += ['    println!("{}", ' + j('cargo:rustc-link-search=native=' + lib.parent.as_posix()) + ');',
                      '    println!("{}", ' + j('cargo:rustc-link-lib=static=' + lib.stem) + ');']
            audit['native'][label] = {'labels': link_deps, 'library': lib.as_posix()}
        for path in [source] + [execution_root / p for p in c['compile_data'] if p.startswith('bazel-out/')]:
            build.append('    println!("{}", ' + j('cargo:rerun-if-changed=' + path.as_posix()) + ');')
        build.append('}')
        write(folder / 'Cargo.toml', '\n'.join(lines) + '\n')
        write(folder / 'build.rs', '\n'.join(build) + '\n')
    # Capture all selected feature sets, including transitive dependencies.
    for label in external:
        audit['feature_variants'][label] = sorted({tuple(v.get('crate_features', {}).get('stringListValue', [])) for v in attrs.get(label, [])})
    write(CACHE / 'audit.json', json.dumps(audit, indent=2) + '\n')
    asset_compiler_inputs = []
    visited_assets = set()
    def asset_inputs(label):
        if label in visited_assets or not label.startswith('@@//kestrel/'):
            return
        visited_assets.add(label)
        c = crates[label]
        asset_compiler_inputs.extend(ROOT / f for f in c['srcs'] if not f.startswith('bazel-out/'))
        for dep in c['deps'] + c['proc_macro_deps']:
            asset_inputs(dep)
    if '@@//kestrel/tools/assetc:assetc' in crates:
        asset_inputs('@@//kestrel/tools/assetc:assetc')
    state = {'inputs': fingerprint(input_files()), 'generated': fingerprint(generated),
             'asset_compiler_inputs': fingerprint(asset_compiler_inputs),
             'external_manifests': fingerprint([folder / 'Cargo.toml' for folder in patches.values()]),
             'metadata': fingerprint([CACHE / 'configured-rust.jsonl', CACHE / 'attributes.json']),
             'toolchain': VERSION, 'created': time.time()}
    write(CACHE / 'state.json', json.dumps(state, indent=2) + '\n')
    settings = json.loads((ROOT / '.rules_rust_analyzer/editor-settings.json').read_text(encoding='utf-8-sig'))
    cargo, rustc = toolchain()
    env = {k: cargo_env()[k] for k in ('RUSTUP_TOOLCHAIN', 'CARGO', 'RUSTC', 'RUSTC_WRAPPER', 'CARGO_TARGET_DIR', 'PATH')}
    editor = {'cmd': [settings['rust-analyzer.server.path']], 'env': env,
              'settings': {'rust-analyzer': {'linkedProjects': [(CACHE / 'Cargo.toml').as_posix()],
                  'cargo': {'targetDir': (CACHE / 'target').as_posix()},
                  'procMacro': {'server': settings['rust-analyzer.procMacro.server']},
                  'rustfmt': {'overrideCommand': settings['rust-analyzer.rustfmt.overrideCommand']},
                  'check': {'command': 'check'}, 'checkOnSave': True}}}
    write(CACHE / 'neovim.json', json.dumps(editor, indent=2) + '\n')
    print(f'Generated {len(first)} Kestrel crates; {len(external)} real external crates. Cache: {CACHE}')


def check_fresh():
    state = json.loads((CACHE / 'state.json').read_text())
    if state['inputs'] != fingerprint(input_files()):
        raise RuntimeError('Kestrel editor metadata/asset recipes changed: rerun cache.py prepare')
    for section in ('generated', 'metadata', 'asset_compiler_inputs', 'external_manifests'):
        for name, digest in state[section].items():
            path = Path(name)
            if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
                raise RuntimeError(f'Kestrel editor cache changed/missing: {name}; rerun prepare')
    print('Kestrel editor cache is fresh.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['prepare', 'status', 'check', 'metadata', 'fetch'])
    parser.add_argument('--from-cache', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--all-kestrel', '--all-game', dest='all_game', action='store_true', help='Include all Kestrel Rust targets after validating the app prototype')
    args = parser.parse_args()
    if args.command == 'prepare':
        prepare(args)
    else:
        check_fresh()
        if args.command in ('check', 'metadata', 'fetch'):
            cargo, _ = toolchain()
            command = [str(cargo), args.command, '--manifest-path', str(CACHE / 'Cargo.toml')]
            if args.command != 'fetch':
                command += ['--offline']
            if args.command == 'metadata':
                command += ['--format-version=1']
            elif args.command == 'check':
                command += ['--message-format=json', '--workspace', '--all-targets']
            elapsed = run(command, CACHE / ('cargo-' + args.command + '.jsonl'), cwd=CACHE, env=cargo_env(), allow_failure=args.command == 'check')
            print(f'{args.command}: {elapsed:.3f}s')
            if args.command == 'check':
                events = [json.loads(l) for l in (CACHE / 'cargo-check.jsonl').read_text(encoding='utf-8').splitlines() if l.strip()]
                if not any(e.get('reason') == 'build-finished' and e['success'] for e in events):
                    for event in events:
                        if event.get('reason') == 'compiler-message' and event['message']['level'] == 'error':
                            print(event['message'].get('rendered', event['message']['message']), file=sys.stderr)
                    sys.exit(1)


if __name__ == '__main__':
    main()
