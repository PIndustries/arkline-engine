#!/usr/bin/env python3
"""Dependency-free fixtures, including real offline temporary Cargo workspaces."""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import check_repository_organization as check


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        rows = []
        for name, directory in check.MEMBERS.items():
            rows.append({'id': name, 'name': name,
                         'manifest_path': str(self.root / directory / 'Cargo.toml'),
                         'dependencies': []})
        self.metadata = {'workspace_root': str(self.root), 'packages': rows,
                         'workspace_members': list(check.MEMBERS)}
        self.packages = {p['name']: p for p in rows}
        for source, target, kind, cfg in check.EDGES:
            self.packages[source]['dependencies'].append({
                'name': target, 'kind': None if kind == 'normal' else kind,
                'target': cfg, 'path': str(self.root / check.MEMBERS[target])})

    def test_exact_policy(self):
        self.assertEqual(check.inspect(self.metadata), [])

    def test_membership(self):
        self.metadata['workspace_members'].pop()
        self.assertTrue(check.inspect(self.metadata))

    def test_location(self):
        self.packages['arkline-client']['manifest_path'] = str(self.root / 'wrong/Cargo.toml')
        self.assertTrue(check.inspect(self.metadata))

    def test_missing_edge(self):
        self.packages['arkline-server']['dependencies'].clear()
        self.assertTrue(check.inspect(self.metadata))

    def test_all_kinds_and_targets(self):
        for kind in (None, 'dev', 'build'):
            for cfg in (None, 'cfg(target_os = "none")'):
                with self.subTest(kind=kind, cfg=cfg):
                    metadata = copy.deepcopy(self.metadata)
                    metadata['packages'][0]['dependencies'].append({
                        'name': 'arkline-client', 'kind': kind, 'target': cfg,
                        'path': str(self.root / 'apps/client')})
                    self.assertTrue(check.inspect(metadata))

    def test_native_exception_not_unconditional(self):
        dep = next(d for d in self.packages['arkline-client']['dependencies'] if d['name'] == 'arkline-server')
        dep['target'] = None
        self.assertTrue(check.inspect(self.metadata))

    def test_wrong_path_or_registry_substitution(self):
        dep = self.packages['arkline-server']['dependencies'][0]
        for path in (None, str(self.root / 'other')):
            dep['path'] = path
            self.assertTrue(check.inspect(self.metadata))

    def test_cli_errors(self):
        with patch.object(check.subprocess, 'run', side_effect=FileNotFoundError('cargo')):
            self.assertEqual(check.main(['--root', str(self.root)]), 2)
        with patch.object(check.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, '{}')):
            self.assertEqual(check.main(['--root', str(self.root)]), 2)
        self.metadata['workspace_members'].pop()
        with patch.object(check.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, json.dumps(self.metadata))):
            self.assertEqual(check.main(['--root', str(self.root)]), 1)

    def test_cargo_command_contract(self):
        with patch.object(check.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, json.dumps(self.metadata))) as run:
            self.assertEqual(check.main(['--root', str(self.root)]), 0)
            self.assertEqual(run.call_args.args[0], ['cargo', 'metadata', '--no-deps', '--offline', '--locked', '--format-version', '1'])

    def make_workspace(self):
        (self.root / 'Cargo.toml').write_text('[workspace]\nresolver="2"\nmembers=' + json.dumps(list(check.MEMBERS.values())) + '\n[workspace.dependencies]\nprotocol_alias={package="arkline-protocol",path="crates/arkline-protocol"}\n')
        for name, directory in check.MEMBERS.items():
            path = self.root / directory
            (path / 'src').mkdir(parents=True)
            (path / 'src/lib.rs').write_text('')
            text = f'[package]\nname="{name}"\nversion="0.1.0"\nedition="2021"\n'
            for source, target, kind, cfg in sorted(check.EDGES, key=repr):
                if source != name:
                    continue
                section = f"target.'{cfg}'.dependencies" if cfg else 'dependencies'
                # Separate table entry syntax allows multiple inherited dependencies.
                alias = 'protocol_alias' if target == 'arkline-protocol' else target.replace('-', '_')
                text += f'\n[{section}.{alias}]\n'
                if target == 'arkline-protocol':
                    text += 'workspace=true\n'
                else:
                    text += f'package="{target}"\npath="{os.path.relpath(self.root / check.MEMBERS[target], path)}"\n'
            (path / 'Cargo.toml').write_text(text)

    @unittest.skipUnless(shutil.which('cargo'), 'cargo required for real metadata fixtures')
    def test_real_cargo_alias_inheritance_target_and_cli(self):
        self.make_workspace()
        self.assertEqual(check.main(['--root', str(self.root)]), 0)
        path = self.root / 'crates/arkline-protocol/Cargo.toml'
        original = path.read_text()
        for section in ('dependencies', 'build-dependencies', 'dev-dependencies',
                        'target.\'cfg(target_os = "none")\'.dependencies',
                        'target.\'cfg(target_os = "none")\'.build-dependencies',
                        'target.\'cfg(target_os = "none")\'.dev-dependencies'):
            with self.subTest(section=section):
                path.write_text(original + f'\n[{section}.bad_alias]\npackage="arkline-asset-tool"\npath="../../apps/asset-tool"\n')
                result = subprocess.run([sys.executable, str(Path(check.__file__).resolve()), '--root', str(self.root)], capture_output=True, text=True)
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn('unapproved direct dependency', result.stderr)

    @unittest.skipUnless(shutil.which('cargo'), 'cargo required for real metadata fixtures')
    def test_no_write_missing_and_outdated_lock(self):
        self.make_workspace()
        lock = self.root / 'Cargo.lock'
        for contents in (None, '# intentionally stale lock\nversion = 4\n'):
            if contents is not None:
                lock.write_text(contents)
            before = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob('*') if p.is_file()}
            self.assertEqual(check.main(['--root', str(self.root)]), 0)
            after = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob('*') if p.is_file()}
            self.assertEqual(before, after)

    @unittest.skipUnless(shutil.which('cargo'), 'cargo required for real metadata fixtures')
    def test_real_metadata_failure_exit(self):
        (self.root / 'Cargo.toml').write_text('invalid toml !')
        self.assertEqual(check.main(['--root', str(self.root)]), 2)


if __name__ == '__main__':
    unittest.main()
