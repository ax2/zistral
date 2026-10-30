"""Windows-only offline development probe for pre-sharing input boundaries.

Python and Windows process counters are development tools, not zi runtime dependencies.
Every trial checks the complete native output or expected failure/file preservation.
"""
import argparse
import ctypes
import ctypes.wintypes
import hashlib
import json
import os
import pathlib
import platform
import statistics
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=pathlib.Path)
    parser.add_argument('--runs', type=int, choices=range(1, 6), default=3)
    parser.add_argument('--output', type=pathlib.Path, help='New JSON report; never overwrite')
    args = parser.parse_args()
    if os.name != 'nt':
        parser.error('This optional development probe uses Windows process counters; zi is cross-platform')
    binary = args.binary.resolve(strict=True)
    if not binary.is_file():
        parser.error('binary must be a regular executable file')
    if args.output and args.output.exists():
        parser.error('output already exists; choose a new report path')

    class Counters(ctypes.Structure):
        _fields_ = [('cb', ctypes.wintypes.DWORD), ('PageFaultCount', ctypes.wintypes.DWORD)] + [
            (name, ctypes.c_size_t) for name in [
                'PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
                'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage', 'QuotaNonPagedPoolUsage',
                'PagefileUsage', 'PeakPagefileUsage', 'PrivateUsage',
            ]
        ]

    get_memory = ctypes.WinDLL('psapi', use_last_error=True).GetProcessMemoryInfo
    get_memory.argtypes = [ctypes.wintypes.HANDLE, ctypes.POINTER(Counters), ctypes.wintypes.DWORD]
    get_memory.restype = ctypes.wintypes.BOOL
    payload = 'x' * (7 * 1024 * 1024)
    flat = ('{"token":"secret","payload":"' + payload + '"}').encode()
    deep = '{"payload":"' + payload + '"}'
    for _ in range(127):
        deep = '{"a":' + deep + '}'
    deep = deep.encode()
    repeated = (b'{"k":"' + b'x' * 64 + b'"}\n') * 100000
    cases = [
        ('shallow-strict', ['data', 'json-redact', '--pointer', '/token'], flat, 'mask'),
        ('shallow-optional', ['data', 'json-redact', '--pointer', '/token', '--if-present'], flat, 'mask'),
        ('deep-optional-missing', ['data', 'json-redact', '--pointer', '/a' * 127 + '/absent', '--if-present'], deep, 'unchanged'),
        ('deep-profile', ['data', 'json-profile'], deep, 'profile'),
        ('100k-profile', ['data', 'jsonl-profile'], repeated, 'records'),
        ('preview-encoded-overflow', ['--preview', '--no-tools', '--model', 'fixture'], b'\0' * (3 * 1024 * 1024), 'overflow'),
    ]
    results = []
    temporary_root = pathlib.Path(tempfile.gettempdir()).resolve()
    with tempfile.TemporaryDirectory(prefix='zistral-sharing-boundary-') as name:
        directory = pathlib.Path(name).resolve()
        if directory.parent != temporary_root or not directory.name.startswith('zistral-sharing-boundary-'):
            raise RuntimeError('Unexpected temporary workspace boundary')
        env = {key: value for key, value in os.environ.items()
               if not key.startswith('ZI_') and key.upper() not in {'OPENAI_API_KEY', 'ANTHROPIC_API_KEY', 'HTTP_PROXY', 'HTTPS_PROXY', 'ALL_PROXY'}}
        env.update(APPDATA=str(directory / 'config'), XDG_CONFIG_HOME=str(directory / 'config'))
        for case, options, data, check in cases:
            source = directory / 'input'
            source.write_bytes(data)
            trials = []
            expected = None
            if check == 'mask':
                expected = flat.replace(b'"token":"secret"', b'"token":"[REDACTED]"') + b'\n'
            elif check == 'unchanged':
                expected = data + b'\n'
            for _ in range(args.runs):
                target = directory / 'keep.json'
                args_case = list(options)
                if check == 'overflow':
                    target.write_bytes(b'prior preview')
                    args_case += ['-o', str(target), '--force', 'task']
                start = time.perf_counter()
                with source.open('rb') as input_file:
                    process = subprocess.Popen([str(binary), *args_case], stdin=input_file,
                                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                               env=env, cwd=directory)
                    try:
                        stdout, stderr = process.communicate(timeout=45)
                    except BaseException:
                        process.kill()
                        process.communicate()
                        raise
                elapsed = time.perf_counter() - start
                if check == 'overflow':
                    if process.returncode == 0 or stdout or b'16 MiB' not in stderr or target.read_bytes() != b'prior preview':
                        raise RuntimeError('Preview overflow did not fail completely while preserving the old file')
                else:
                    if process.returncode != 0 or stderr:
                        raise RuntimeError(f'{case} failed instead of returning the complete native result')
                    if expected is not None and stdout != expected:
                        raise RuntimeError(f'{case} output differed from the lexical reference')
                    if check == 'profile':
                        profile = json.loads(stdout)
                        pointers = [''] + ['/a' * n for n in range(1, 128)] + ['/a' * 127 + '/payload']
                        paths = profile['paths']
                        if profile['records'] != 1 or [item['pointer'] for item in paths] != sorted(pointers):
                            raise RuntimeError('Deep profile pointer set differs')
                        for item in paths:
                            kind = 'string' if item['pointer'].endswith('/payload') else 'object'
                            if item['present'] != 1 or item['missing'] != 0 or item['types'] != {kind: 1}:
                                raise RuntimeError('Deep profile count differs')
                    elif check == 'records':
                        expected_profile = {'records': 100000, 'paths': [
                            {'pointer': '', 'present': 100000, 'missing': 0, 'types': {'object': 100000}},
                            {'pointer': '/k', 'present': 100000, 'missing': 0, 'types': {'string': 100000}},
                        ]}
                        if json.loads(stdout) != expected_profile:
                            raise RuntimeError('100k record profile differs')
                counters = Counters()
                counters.cb = ctypes.sizeof(counters)
                if not get_memory(int(process._handle), ctypes.byref(counters), ctypes.sizeof(counters)):
                    raise ctypes.WinError(ctypes.get_last_error())
                if not counters.PeakWorkingSetSize or not counters.PeakPagefileUsage:
                    raise RuntimeError('Peak process counters unavailable')
                trials.append({'elapsed_seconds': round(elapsed, 6),
                               'peak_working_set_bytes': counters.PeakWorkingSetSize,
                               'peak_pagefile_usage_bytes': counters.PeakPagefileUsage})
            if (directory / 'config').exists():
                raise RuntimeError('Offline checks unexpectedly created configuration')
            results.append({'case': case, 'input_bytes': len(data), 'output_bytes': len(stdout),
                            'output_sha256': hashlib.sha256(stdout).hexdigest(), 'trials': trials,
                            'median_elapsed_seconds': statistics.median(t['elapsed_seconds'] for t in trials),
                            'median_peak_working_set_bytes': statistics.median(t['peak_working_set_bytes'] for t in trials),
                            'median_peak_pagefile_usage_bytes': statistics.median(t['peak_pagefile_usage_bytes'] for t in trials),
                            'verified': True})
    report = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'binary_bytes': binary.stat().st_size, 'platform': platform.platform(),
              'runs_per_case': args.runs, 'cases': results,
              'scope': 'Synthetic Windows whole-process OS counters and elapsed time, including startup/input/output. '
                       'No model/network requests. This is not an allocator-only measurement, official-release '
                       'installation check, cross-platform comparison or general performance guarantee. '
                       'Terminal display expansion is outside this pipe/file probe.'}
    rendered = json.dumps(report, indent=2) + '\n'
    if args.output:
        with args.output.open('x', encoding='utf-8') as output:
            output.write(rendered)
    else:
        print(rendered, end='')


if __name__ == '__main__':
    main()
