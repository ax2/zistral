"""Create native installers plus portable archives using OS-provided tools."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib
import zipfile

root = Path(__file__).resolve().parents[1]
os.chdir(root)
parser = argparse.ArgumentParser()
parser.add_argument('--target', required=True)
parser.add_argument('--binary', type=Path)
args = parser.parse_args()
version = tomllib.loads((root / 'Cargo.toml').read_text())['package']['version']
tag = 'v' + version
assert os.environ.get('RELEASE_TAG', tag) == tag
binary_name = 'zi.exe' if os.name == 'nt' else 'zi'
binary = (args.binary or root / 'target' / args.target / 'release' / binary_name).resolve()
assert subprocess.check_output([str(binary), '--version'], text=True).strip() == 'zi ' + version
count = json.loads(subprocess.check_output([str(binary), '--count'], input='你好'.encode()))
assert count['characters'] == 2 and count['cjk_characters'] == 2
dist = root / 'dist'
dist.mkdir(exist_ok=True)
name = f'zistral-{tag}-{args.target}'
package = dist / name
package.mkdir(exist_ok=True)
for source in [binary, root/'README.md', root/'README.zh-CN.md', root/'LICENSE']:
    shutil.copy2(source, package / source.name)
if os.name == 'nt':
    archive = dist / (name + '.zip')
    with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as z:
        for p in sorted(package.iterdir()): z.write(p, name + '/' + p.name)
    compiler = shutil.which('iscc') or r'C:\Program Files (x86)\Inno Setup 6\ISCC.exe'
    subprocess.run([compiler, '/Qp', f'/DVersion={version}', f'/DBinaryPath={binary}',
                    f'/DRepoRoot={root}', f'/DOutputPath={dist}', str(root/'packaging/windows.iss')], check=True)
else:
    archive = dist / (name + '.tar.gz')
    with tarfile.open(archive, 'w:gz') as t: t.add(package, arcname=name)
    native = dist / ('native-' + args.target)
    if native.exists(): shutil.rmtree(native)
    bindir = native / 'usr' / ('local/bin' if 'apple' in args.target else 'bin')
    bindir.mkdir(parents=True)
    shutil.copy2(binary, bindir/'zi')
    (bindir/'zi').chmod(0o755)
    if 'apple' in args.target:
        subprocess.run(['pkgbuild', '--root', str(native), '--identifier', 'io.github.ax2.zistral',
                        '--version', version, '--install-location', '/',
                        str(dist/f'zistral-{tag}-macos-arm64.pkg')], check=True)
    else:
        doc = native/'usr/share/doc/zistral'
        doc.mkdir(parents=True)
        for name in ['README.md', 'README.zh-CN.md', 'LICENSE']:
            shutil.copy2(root/name, doc/name)
        debian = native/'DEBIAN'
        debian.mkdir()
        (debian/'control').write_text(f"""Package: zistral
Version: {version}
Section: utils
Priority: optional
Architecture: amd64
Maintainer: ax2 <zistral@users.noreply.github.com>
Depends: libc6 (>= 2.35), ca-certificates
Homepage: https://github.com/ax2/zistral
Description: Lightweight pipe-native command-line agent (zi)
 Single binary with interactive setup and model configuration import.
""")
        subprocess.run(['dpkg-deb', '--root-owner-group', '--build', str(native),
                        str(dist/f'zistral-{tag}-linux-amd64.deb')], check=True)
for p in sorted(dist.iterdir()):
    if p.is_file(): print(p.name, hashlib.sha256(p.read_bytes()).hexdigest())
