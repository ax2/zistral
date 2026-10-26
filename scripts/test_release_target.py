import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import unittest

from release_target import resolve_release


class ReleaseTargetTests(unittest.TestCase):
    def test_ordinary_branch_commits_cannot_publish_a_new_version(self):
        for branch in ['main', 'feature/new-cli', 'release', 'release/v0.16.20', 'release/v0.16.1', 'release/v0.16.2/extra']:
            with self.subTest(branch=branch), self.assertRaises(ValueError):
                resolve_release('0.16.2', 'push', 'branch', branch)

    def test_explicit_release_branch_and_tag_must_match_package(self):
        self.assertEqual(resolve_release('0.16.2', 'push', 'branch', 'release/v0.16.2'), 'v0.16.2')
        self.assertEqual(resolve_release('0.16.2', 'push', 'tag', 'v0.16.2'), 'v0.16.2')
        for ref in ['v0.16.1', 'latest', 'v0.16.2\n', 'release/v0.16.2']:
            with self.subTest(ref=ref), self.assertRaises(ValueError):
                resolve_release('0.16.2', 'push', 'tag', ref)

    def test_manual_dispatch_requires_the_exact_tag_and_valid_source(self):
        for source_type, source in [('branch', 'main'), ('branch', 'release/v0.16.2'), ('tag', 'v0.16.2')]:
            self.assertEqual(resolve_release('0.16.2', 'workflow_dispatch', source_type, source, 'v0.16.2'), 'v0.16.2')
        for requested in ['', 'latest', 'v0.16.1', '0.16.2', 'v0.16.2\n']:
            with self.subTest(requested=requested), self.assertRaises(ValueError):
                resolve_release('0.16.2', 'workflow_dispatch', 'branch', 'main', requested)
        with self.assertRaises(ValueError):
            resolve_release('0.16.2', 'workflow_dispatch', 'tag', 'v0.16.1', 'v0.16.2')
        with self.assertRaises(ValueError):
            resolve_release('0.16.2', 'workflow_dispatch', 'branch', '', 'v0.16.2')

    def test_other_events_and_nonstable_versions_are_rejected(self):
        for event in ['pull_request', 'schedule', 'workflow_run', 'repository_dispatch', '']:
            with self.subTest(event=event), self.assertRaises(ValueError):
                resolve_release('0.16.2', event, 'branch', 'release/v0.16.2')
        for version in ['0.16', '0.16.2-beta', '0.16.2+build', '00.16.2', '0.16.2\n', '../0.16.2', '0.16.2٣']:
            with self.subTest(version=version), self.assertRaises(ValueError):
                resolve_release(version, 'push', 'branch', 'release/v' + version)
        with self.assertRaises(ValueError):
            resolve_release('0.16.2', 'push', '', 'release/v0.16.2')

    def test_cli_rejects_before_workflow_output_and_accepts_an_explicit_release(self):
        script = Path(__file__).with_name('release_target.py')
        version = tomllib.loads((script.parent.parent / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
        with tempfile.TemporaryDirectory() as root:
            output = Path(root) / 'outputs.txt'
            output.write_bytes(b'prior=value\n')
            env = {**os.environ, 'GITHUB_OUTPUT': str(output)}
            rejected = subprocess.run([sys.executable, str(script), '--event', 'push', '--ref-type', 'branch', '--ref-name', 'main'], env=env, capture_output=True)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertEqual(rejected.stdout, b'')
            self.assertEqual(output.read_bytes(), b'prior=value\n')
            accepted = subprocess.run([sys.executable, str(script), '--event', 'push', '--ref-type', 'branch', '--ref-name', 'release/v' + version], env=env, capture_output=True)
            self.assertEqual(accepted.returncode, 0, accepted.stderr)
            self.assertEqual(accepted.stdout.strip(), ('v' + version).encode())
            self.assertEqual(output.read_bytes(), b'prior=value\ntag=v' + version.encode() + b'\n')


if __name__ == '__main__':
    unittest.main()
