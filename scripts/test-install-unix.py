"""Verify native/portable Unix candidates on a clean disposable CI runner.

Refuses existing zi payloads, package receipts or model configuration. Uses only
the standard library on the test host; it is not an end-user installer or runtime.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tarfile
import tempfile

from upgrade_baseline import verify as verified_baseline
from test_service_setup_delivery import verify_legacy_budget_cleanup


IDENTIFIER = 'io.github.ax2.zistral'
TARGETS = {'x86_64-unknown-linux-gnu': ('linux', Path('/usr/bin/zi'), 'linux-amd64.deb'),
           'aarch64-apple-darwin': ('darwin', Path('/usr/local/bin/zi'), 'macos-arm64.pkg')}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def invoke(args, check=True):
    return subprocess.run(list(map(str, args)), check=check, capture_output=True, timeout=180)


def portable_binary(archive, destination, target, version, expected_digest):
    """Validate the entire archive; copy just its verified binary to a test dir."""
    name = f'zistral-v{version}-{target}'
    expected = {name, *(name + '/' + p for p in ['zi', 'README.md', 'README.zh-CN.md', 'LICENSE'])}
    with tarfile.open(archive, 'r:gz') as tar:
        members = tar.getmembers()
        assert len(members) == len(expected) and {m.name for m in members} == expected, 'unexpected portable archive members'
        for item in members:
            if item.name == name:
                assert item.isdir(), 'portable root must be a directory'
            else:
                assert item.isfile(), 'portable payload must contain only regular files'
                assert 0 < item.size <= 16 * 1024 * 1024, 'invalid portable file size'
        member = tar.getmember(name + '/zi')
        assert member.mode & 0o100 and not member.mode & 0o7000, 'portable zi must be owner-executable without special bits'
        data = tar.extractfile(member).read()
        assert hashlib.sha256(data).hexdigest() == expected_digest, 'portable binary differs from build'
    executable = destination / 'zi'
    executable.write_bytes(data)
    executable.chmod(0o755)
    return executable


def receipt(target):
    if TARGETS[target][0] == 'linux':
        return invoke(['dpkg-query', '-W', '-f=${Version}', 'zistral'], check=False)
    return invoke(['pkgutil', '--pkg-info', IDENTIFIER], check=False)


def ensure_fresh(installed, target, config):
    assert not installed.exists() and not installed.is_symlink(), 'preserve existing zi executable; use a clean CI runner'
    assert receipt(target).returncode != 0, 'preserve existing package receipt; use a clean CI runner'
    assert not config.parent.exists() and not config.parent.is_symlink(), 'preserve existing model settings; use a clean CI runner'


def configuration_links(binary):
    """Read linked settings, but never replace links during an explicit save."""
    environment = {key: value for key, value in os.environ.items()
                   if not key.upper().startswith('ZI_') and key.upper() not in {'OPENAI_API_KEY', 'ANTHROPIC_API_KEY'}}
    original = b"model='root'\nbase_url='http://127.0.0.1:1/v1'\napi_key='SYNTHETIC_LINK_KEY_993'\nactive_profile='work'\n[profiles.work]\nmodel='work'\napi_key='SYNTHETIC_LINK_KEY_993'\napi='chat'\nmax_output_tokens=128\n[profiles.other]\nmodel='other'\n"
    writers = [['use', 'other'], ['config', '--rename', 'other', 'renamed'],
               ['config', '--remove', 'other'], ['config', '--name', 'work', '--max-output-tokens', '256'],
               ['config', '--name', 'work', '--clear-output-budget'],
               ['config', '--name', 'work', '--api-key-env', 'ZISTRAL_LINK_REF_993'],
               ['config', '--name', 'default', '--clear-saved-key']]
    with tempfile.TemporaryDirectory(prefix='zi linked config-测试 ') as temporary:
        root = Path(temporary)
        target, alias = root / 'tracked.toml', root / 'config.toml'
        target.write_bytes(original)
        alias.symlink_to(target.name)
        for arguments in [['config', '--list'], ['models'], ['doctor', '--json']]:
            output = subprocess.run([str(binary), '--config', str(alias), *arguments],
                                    input=b'', capture_output=True, env=environment, timeout=20)
            assert output.returncode == 0, 'reading a linked configuration must still work'
            assert b'SYNTHETIC_LINK_KEY_993' not in output.stdout + output.stderr
        for arguments in writers:
            output = subprocess.run([str(binary), '--config', str(alias), *arguments],
                                    input=b'', capture_output=True, env=environment, timeout=20)
            assert output.returncode != 0, 'saving through a symbolic link must fail explicitly'
            assert b'actual regular file path' in output.stderr and not output.stdout
            assert b'SYNTHETIC_LINK_KEY_993' not in output.stderr
            assert alias.is_symlink() and os.readlink(alias) == target.name, 'configuration link was replaced'
            assert target.read_bytes() == original and alias.read_bytes() == original, 'linked settings changed'
            assert {p.name for p in root.iterdir()} == {'tracked.toml', 'config.toml'}, 'unexpected save artifact'
    return {'linked_readers': 3, 'rejected_link_writers': len(writers), 'links_and_target_preserved': True,
            'real_provider_requests': 0}


def delivery(binary, scripts):
    smoke = subprocess.run([str(binary), '--count'], input=b'hello world\n',
                           capture_output=True, check=True, timeout=20)
    assert json.loads(smoke.stdout)['words'] == 2 and not smoke.stderr, 'installed pipe count failed'
    expected = [('test_pre_sharing_delivery.py', 8), ('test_table_preparation_delivery.py', 8),
                ('test_service_setup_delivery.py', 40)]
    result = []
    for script, count in expected:
        completed = invoke([sys.executable, scripts / script, '--binary', binary])
        report = json.loads(completed.stdout)
        assert report['passed'] == count, 'incomplete installed workflow report'
        if script == 'test_service_setup_delivery.py':
            assert report['synthetic_loopback_requests'] == 24 and report['real_provider_requests'] == 0
        result.append(report)
    return result


def verify(binary, dist, target, baseline_assets=None):
    if sys.flags.optimize:
        raise RuntimeError('Run installer verification without -O; safety checks require assertions')
    platform, installed, suffix = TARGETS[target]
    assert sys.platform == platform, 'run this verifier only on its matching disposable Unix runner'
    version_text = invoke([binary, '--version']).stdout.decode().strip()
    assert re.fullmatch(r'zi \d+\.\d+\.\d+', version_text), 'invalid candidate version'
    version = version_text.split()[1]
    expected_digest = digest(binary)
    native = dist / f'zistral-v{version}-{suffix}'
    archive = dist / f'zistral-v{version}-{target}.tar.gz'
    assert native.is_file() and archive.is_file(), 'missing native/portable candidates'
    config_home = Path(os.environ.get('XDG_CONFIG_HOME') or Path.home() / '.config')
    config = config_home / 'zistral/config.toml'
    ensure_fresh(installed, target, config)
    baseline = verified_baseline(baseline_assets, target) if baseline_assets is not None else None
    owned_digests = {expected_digest, *([baseline['binary_sha256']] if baseline else [])}
    sentinel = b"# Stable-compatible upgrade fixture; formatting must survive.\nmodel='installer-sentinel'\nbase_url='http://127.0.0.1:1/v1'\napi_key='SYNTHETIC_INSTALL_SENTINEL_993'\nactive_profile='work'\n[profiles.work]\nmodel='stable-work-model'\nbase_url='http://127.0.0.1:1/v1'\napi='responses'\napi_key_env='ZISTRAL_UPGRADE_UNSET_993'\napi_key='SYNTHETIC_WORK_SENTINEL_993'\nendpoint='http://127.0.0.1:1/v1/responses'\n[profiles.other]\nmodel='stable-other-model'\napi_key='SYNTHETIC_OTHER_SENTINEL_993'\n"
    checks, native_reports, portable_reports = [], [], []
    link_reports = []
    rollback = None
    attempted = False
    scripts = Path(__file__).resolve().parent
    # The directory is verified absent; remove only this known file/empty dir.
    config.parent.mkdir(parents=True)
    config.write_bytes(sentinel)
    config.chmod(0o600)
    try:
        with tempfile.TemporaryDirectory(prefix='zi portable-测试 ') as temporary:
            portable = portable_binary(archive, Path(temporary), target, version, expected_digest)
            assert invoke([portable, '--version']).stdout.decode().strip() == version_text
            portable_reports = delivery(portable, scripts)
            link_reports.append(configuration_links(portable))
            checks.append('portable archive: exact members, executable bytes/mode, Unicode path and all8+7+40 workflow checks')
        if baseline:
            attempted = True
            if platform == 'linux':
                invoke(['sudo', 'dpkg', '-i', baseline['installer']])
                assert receipt(target).stdout.decode().strip() == baseline['version']
            else:
                invoke(['sudo', 'installer', '-pkg', baseline['installer'], '-target', '/'])
            assert installed.is_file() and not installed.is_symlink() and digest(installed) == baseline['binary_sha256'], 'published executable differs from pinned release'
            assert invoke([installed, '--version']).stdout.decode().strip() == 'zi ' + baseline['version']
            assert 'stable-work-model' in invoke([installed, '--config', config, 'models']).stdout.decode(), 'published version could not read legacy fixture'
            assert config.read_bytes() == sentinel, 'published installer changed configuration'
            if baseline['version'] == '0.33.0':
                rollback = verify_legacy_budget_cleanup(binary, installed)
        for _ in range(2):
            attempted = True
            if platform == 'linux':
                invoke(['sudo', 'dpkg', '-i', native])
                assert receipt(target).stdout.decode().strip() == version
            else:
                invoke(['sudo', 'installer', '-pkg', native, '-target', '/'])
                assert receipt(target).returncode == 0
            assert installed.is_file() and not installed.is_symlink() and digest(installed) == expected_digest, 'installed executable differs from build'
            assert invoke([installed, '--version']).stdout.decode().strip() == version_text
            assert 'stable-work-model' in invoke([installed, '--config', config, 'models']).stdout.decode()
            native_reports.append(delivery(installed, scripts))
            link_reports.append(configuration_links(installed))
            assert config.read_bytes() == sentinel and config.stat().st_mode & 0o777 == 0o600, 'model configuration changed during install/upgrade'
        checks.append('native install/reinstall: exact build bytes and all8+7+40 workflows twice')
        if baseline:
            checks.append('pinned published release to candidate: exact old/new executable digests and unchanged complete legacy configuration')
    finally:
        # Fresh-runner guards and digest checks prevent removing somebody else's
        # payload if a failed test leaves unexpected filesystem state.
        if attempted and installed.exists():
            assert not installed.is_symlink() and digest(installed) in owned_digests, 'preserve unexpected installed executable; cleanup refused'
            if platform == 'linux':
                invoke(['sudo', 'dpkg', '-r', 'zistral'])
            else:
                invoke(['sudo', 'rm', str(installed)])
                if receipt(target).returncode == 0:
                    invoke(['sudo', 'pkgutil', '--forget', IDENTIFIER])
        assert not installed.exists() and not installed.is_symlink(), 'native payload not removed'
        assert config.read_bytes() == sentinel, 'preserve modified configuration; cleanup refused'
        config.unlink()
        config.parent.rmdir()
    checks.append('uninstall: owned payload removed; model configuration preserved before isolated fixture cleanup')
    if platform == 'darwin':
        assert receipt(target).returncode != 0, 'native receipt not removed'
    assert len(checks) == (4 if baseline else 3) and len(native_reports) == 2
    return {'passed': len(checks), 'checks': checks, 'target': target, 'version': version,
            'binary_sha256': expected_digest, 'native_installs': 2,
            'portable_workflows': [v['passed'] for v in portable_reports],
            'native_workflows': [[v['passed'] for v in group] for group in native_reports],
            'configuration_link_checks': link_reports,
            'output_budget_rollback': rollback,
            'real_provider_requests': 0, 'configuration_preserved': True,
            'published_baseline': baseline, 'baseline_installs': int(baseline is not None),
            'package_version_changed': baseline is not None and baseline['version'] != version,
            'scope': 'Published-release to candidate replacement when requested; same-version reinstall. No GitHub Release published.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--dist', type=Path, required=True)
    parser.add_argument('--target', choices=TARGETS, required=True)
    parser.add_argument('--baseline-assets', type=Path)
    args = parser.parse_args()
    print(json.dumps(verify(args.binary.resolve(strict=True), args.dist.resolve(strict=True), args.target, args.baseline_assets), ensure_ascii=True))
