"""Verify generated crate edges, versions, features and test-target coverage."""
import json
from pathlib import Path
import tomllib

root = Path(__file__).resolve().parents[2]
cache = root / '.rules_rust_analyzer/cargo-ide'
audit = json.loads((cache / 'audit.json').read_text(encoding='utf-8'))
metadata = json.loads((cache / 'cargo-metadata.jsonl').read_text(encoding='utf-8'))
packages = {p['id']: p for p in metadata['packages']}
nodes = {n['id']: n for n in metadata['resolve']['nodes']}
by_version = {(p['name'], p['version']): p for p in packages.values()}
lock = tomllib.loads((root / 'Cargo.lock').read_text(encoding='utf-8'))
locked = {(p['name'], p['version']) for p in lock['package']}
problems = []
for label, expected in audit['packages'].items():
    key = (expected['name'], expected['version'])
    if key not in locked:
        problems.append(f'Not in Bazel dependency lock: {key}')
    if key not in by_version:
        problems.append(f'Missing Cargo package: {key}')
        continue
    actual = nodes[by_version[key]['id']]
    data = tomllib.loads(Path(expected['manifest']).read_text(encoding='utf-8'))
    declared = set(data.get('features', {}))
    suppressed = {f[4:] for values in data.get('features', {}).values() for f in values if f.startswith('dep:')}
    declared.update(k for k, v in data.get('dependencies', {}).items()
                    if isinstance(v, dict) and v.get('optional') and k not in suppressed)
    wanted = {f for variant in audit['feature_variants'][label] for f in variant}
    missing = (wanted & declared) - set(actual['features'])
    if missing:
        problems.append(f'Missing Cargo-valid features {key}: {sorted(missing)}')
rows = [json.loads(l) for l in (cache / 'configured-rust.jsonl').read_text(encoding='utf-8').splitlines() if l.strip()]
roots = {r['label']: r for r in rows if r['label'] == r['owner'] and r['label'].startswith('@@//kestrel/')}
for label, expected in audit['game_targets'].items():
    p = by_version[(expected['package'], '0.0.0')]
    actual_deps = {d['name']: packages[d['pkg']]['name'] for d in nodes[p['id']]['deps']}
    contexts = [roots[label]] + [roots[t] for t in audit['unit_test_targets'].get(label, [])]
    for crate in contexts:
        for dep in crate['deps'] + crate['proc_macro_deps']:
            name = crate['aliases'].get(dep)
            dep_expected = audit['game_targets'].get(dep, audit['packages'].get(dep))
            assert dep_expected, dep
            if not name:
                candidates = [r for r in rows if r['label'] == dep and r['owner'] == dep]
                name = candidates[0]['name']
            package_name = dep_expected.get('package', dep_expected.get('name'))
            if actual_deps.get(name) != package_name:
                problems.append(f'Wrong/missing edge: {crate["label"]} -> {name} ({package_name})')
covered = set(audit['game_targets']) | {t for ts in audit['unit_test_targets'].values() for t in ts}
if audit['scope'] == '//kestrel/...' and set(roots) != covered:
    problems.append(f'Unmapped game Rust targets: {sorted(set(roots) - covered)}')
report = {'scope': audit['scope'], 'kestrel_packages': audit['first_party_crates'],
          'covered_bazel_targets': len(covered), 'external_crates': len(audit['packages']),
          'feature_support_crates': audit['feature_support_crates'], 'problems': problems}
(cache / 'fidelity-results.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
print(json.dumps(report))
assert not problems, '\n'.join(problems)
