"""Check the Windows payload's imported DLLs using the build host's DUMPBIN.

Build/test tooling only. Neither Python nor Microsoft build tools ship with zi.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys


# Reviewed imports of the standalone x64 release. Any new DLL needs review;
# silently adding a redistributable, OpenSSL or language runtime is not allowed.
SYSTEM_DLLS = frozenset({
    'advapi32.dll', 'api-ms-win-core-synch-l1-2-0.dll', 'bcrypt.dll',
    'bcryptprimitives.dll', 'kernel32.dll', 'ntdll.dll', 'userenv.dll', 'ws2_32.dll',
})


def checked_imports(output):
    # DUMPBIN module rows have four spaces, unlike section headers and hex
    # summary columns. Check all names, including spaces and non-DLL suffixes.
    imports = sorted({match.group(1).lower() for line in output.splitlines()
                      if (match := re.fullmatch(r' {4}(\S.*?)\s*', line, re.IGNORECASE))})
    if not imports or 'kernel32.dll' not in imports:
        raise ValueError('cannot establish Windows executable imports; packaging refused')
    if set(imports) - SYSTEM_DLLS:
        raise ValueError('unreviewed Windows DLL dependencies; standalone packaging refused')
    return imports


def find_dumpbin(explicit=None):
    if explicit is not None:
        if not Path(explicit).is_file():
            raise RuntimeError('Microsoft DUMPBIN was not found on the build host')
        return str(Path(explicit).resolve())
    found = shutil.which('dumpbin.exe')
    if found:
        return found
    vswhere = Path(os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')) / 'Microsoft Visual Studio/Installer/vswhere.exe'
    if vswhere.is_file():
        result = subprocess.run([str(vswhere), '-latest', '-utf8', '-products', '*',
                                 '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64',
                                 '-find', r'VC\Tools\MSVC\**\bin\Hostx64\x64\dumpbin.exe'],
                                capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=15)
        if result.returncode == 0:
            for line in result.stdout.splitlines():
                if Path(line.strip()).is_file():
                    return line.strip()
    raise RuntimeError('Microsoft DUMPBIN was not found; use a C++ build host or --dumpbin PATH')


def verify(binary, dumpbin=None):
    binary = Path(binary).resolve(strict=True)
    tool = find_dumpbin(dumpbin)
    result = subprocess.run([tool, '/nologo', '/dependents', str(binary)], capture_output=True,
                            text=True, encoding='utf-8', errors='replace', timeout=30)
    if result.returncode != 0:
        raise RuntimeError('Microsoft DUMPBIN could not inspect the executable; packaging refused')
    imports = checked_imports(result.stdout)
    return {'kind': 'windows-runtime-check', 'ok': True, 'target': 'x86_64-pc-windows-msvc',
            'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
            'imports': imports, 'scope': 'Reviewed direct PE imports only; not an OS-compatibility or dynamic-loading proof.'}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--dumpbin', type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(verify(args.binary, args.dumpbin), sort_keys=True))
    except (OSError, ValueError, RuntimeError, subprocess.TimeoutExpired) as error:
        print('Windows runtime dependency check failed: ' + str(error), file=sys.stderr)
        raise SystemExit(1)
