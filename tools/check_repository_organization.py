#!/usr/bin/env python3
"""Check exact workspace membership, locations and declared direct internal edges.

Cargo resolves aliases and workspace inheritance; no dependency fetch or compilation.
Normal, build, dev and target-specific declarations are all checked, even inactive ones.
"""
import argparse
import json
from pathlib import Path
import subprocess
import sys

MEMBERS = {
    'arkline-protocol': 'crates/arkline-protocol',
    **{f'arkline-{name}': f'apps/{name}' for name in
       ('server', 'client', 'verifier', 'cli', 'asset-tool', 'editor')},
}
NATIVE = 'cfg(not(target_arch = "wasm32"))'
EDGES = {
    ('arkline-server', 'arkline-protocol', 'normal', None),
    ('arkline-server', 'arkline-verifier', 'normal', None),
    ('arkline-verifier', 'arkline-protocol', 'normal', None),
    ('arkline-client', 'arkline-protocol', 'normal', None),
    ('arkline-client', 'arkline-server', 'normal', NATIVE),
    ('arkline-client', 'arkline-verifier', 'normal', NATIVE),
    ('arkline-cli', 'arkline-protocol', 'normal', None),
    ('arkline-editor', 'arkline-client', 'normal', None),
    ('arkline-editor', 'arkline-protocol', 'normal', None),
}


def inspect(metadata):
    root = Path(metadata['workspace_root']).resolve()
    packages = {p['id']: p for p in metadata['packages']}
    members = [packages[i] for i in metadata['workspace_members']]
    errors = []
    names = [p['name'] for p in members]
    if set(names) != set(MEMBERS) or len(names) != len(MEMBERS):
        errors.append(f'workspace members differ: expected {sorted(MEMBERS)}, got {sorted(names)}')
    paths = {(root / path).resolve(): name for name, path in MEMBERS.items()}
    actual = set()
    for package in members:
        name = package['name']
        directory = Path(package['manifest_path']).resolve().parent
        if name in MEMBERS and directory != (root / MEMBERS[name]).resolve():
            errors.append(f'{name}: expected location {MEMBERS[name]}, got {directory}')
        for dep in package['dependencies']:
            target = dep['name']  # Cargo's real package name, not the local alias.
            path = Path(dep['path']).resolve() if dep.get('path') else None
            if target not in MEMBERS and path not in paths:
                continue
            if target not in MEMBERS or path != (root / MEMBERS[target]).resolve():
                errors.append(f'{name}: internal dependency {target} must use its canonical workspace path, got {path}')
            actual.add((name, target, dep.get('kind') or 'normal', dep.get('target')))
    for edge in sorted(actual - EDGES, key=repr):
        errors.append(f'unapproved direct dependency: {edge}')
    for edge in sorted(EDGES - actual, key=repr):
        errors.append(f'missing approved direct dependency: {edge}')
    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args(argv)
    try:
        result = subprocess.run(
            ['cargo', 'metadata', '--no-deps', '--offline', '--locked', '--format-version', '1'],
            cwd=args.root, check=True, text=True, capture_output=True,
        )
        errors = inspect(json.loads(result.stdout))
    except (OSError, subprocess.CalledProcessError, ValueError, KeyError, TypeError) as exc:
        print(f'repository organization: metadata failed: {exc}', file=sys.stderr)
        if isinstance(exc, subprocess.CalledProcessError):
            print(exc.stderr, file=sys.stderr)
        return 2
    if errors:
        print('\n'.join(errors), file=sys.stderr)
        return 1
    print('Repository organization: exact workspace membership, locations and direct dependencies passed.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
