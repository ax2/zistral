"""Prepare a pinned public-release installer for disposable CI upgrade checks.

This test-host helper never installs anything. Offline verification is the default;
--download explicitly invokes authenticated gh without exposing its token or logs.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys

BASELINE = json.loads(Path(__file__).with_name('stable-upgrade-baseline.json').read_text())


def verify(assets, target, download=False):
    if sys.flags.optimize:
        raise RuntimeError('Run baseline verification without -O')
    entry = BASELINE['targets'].get(target)
    if entry is None:
        raise ValueError('unsupported upgrade baseline target')
    name = entry['installer']
    if Path(name).name != name or name.startswith('-'):
        raise ValueError('baseline installer must be a plain filename')
    assets = Path(assets)
    if assets.is_symlink():
        raise ValueError('preserve symbolic-link asset directory')
    if download:
        # No clobber/retry/latest lookup: reviewed metadata fixes this release.
        assets.mkdir(parents=True, exist_ok=False)
        try:
            result = subprocess.run(['gh', 'release', 'download', BASELINE['tag'],
                '--repo', 'ax2/zistral', '--pattern', name, '--dir', str(assets)],
                capture_output=True, timeout=60)
        except (OSError, subprocess.TimeoutExpired):
            raise RuntimeError('baseline download failed; no installation attempted') from None
        if result.returncode:
            raise RuntimeError('baseline download failed; no installation attempted')
    installer = assets / name
    if installer.is_symlink() or not installer.is_file():
        raise ValueError('missing regular baseline installer')
    if installer.stat().st_size != entry['bytes']:
        raise ValueError('baseline installer size mismatch; no installation attempted')
    if hashlib.sha256(installer.read_bytes()).hexdigest() != entry['sha256']:
        raise ValueError('baseline installer digest mismatch; no installation attempted')
    return {'version': BASELINE['version'], 'tag': BASELINE['tag'],
            'source_commit': BASELINE['source_commit'],
            'installer': str(installer.resolve(strict=True)),
            'installer_sha256': entry['sha256'], 'binary_sha256': entry['binary_sha256']}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--assets', type=Path, required=True)
    parser.add_argument('--target', choices=BASELINE['targets'], required=True)
    parser.add_argument('--download', action='store_true')
    args = parser.parse_args()
    print(json.dumps(verify(args.assets, args.target, args.download)))
