"""Windows-only development measurement of bounded offline CLI process memory.

Build comparable release binaries first, then run for each binary and key mode:
  python scripts/benchmark_jsonl_memory.py path/to/zi.exe --keys repeated --output before.json
  python scripts/benchmark_jsonl_memory.py path/to/zi.exe --keys unique --output unique.json
Python is a development-only requirement. No provider, configuration, build or download.
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


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('binary', type=pathlib.Path)
    parser.add_argument('--keys', choices=['repeated', 'unique'], default='repeated')
    parser.add_argument('--runs', type=int, choices=range(1, 11), default=3)
    parser.add_argument('--output', type=pathlib.Path, help='New JSON file, otherwise stdout')
    args = parser.parse_args()
    if os.name != 'nt':
        parser.error('This optional measurement uses Windows process counters; zi itself is cross-platform')
    if args.output and args.output.exists():
        parser.error('output already exists; choose a new file')
    binary = args.binary.resolve(strict=True)

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
    unique = args.keys == 'unique'
    keys = [f'{n:064d}' for n in range(100000)] if unique else ['x' * 64] * 100000
    data = ''.join('{"k":"' + key + '"}\n' for key in keys).encode()
    counted = (''.join('{"value":"' + key + '","count":1}\n' for key in keys) if unique else
               '{"value":"' + keys[0] + '","count":100000}\n').encode()
    deduped = data if unique else ('{"k":"' + keys[0] + '"}\n').encode()
    results = []
    temporary_root = pathlib.Path(tempfile.gettempdir()).resolve()
    with tempfile.TemporaryDirectory(prefix='zistral-jsonl-memory-') as name:
        directory = pathlib.Path(name).resolve()
        if directory.parent != temporary_root or not directory.name.startswith('zistral-jsonl-memory-'):
            raise RuntimeError('Unexpected temporary workspace boundary')
        source = directory / 'input.jsonl'
        source.write_bytes(data)
        env = {key: value for key, value in os.environ.items()
               if not key.startswith('ZI_') and key.upper() not in {'OPENAI_API_KEY', 'ANTHROPIC_API_KEY'}}
        env.update(APPDATA=str(directory / 'config'), XDG_CONFIG_HOME=str(directory / 'config'))
        for operation, expected in [('jsonl-count-by', counted), ('jsonl-dedupe', deduped), ('jsonl-sort', data)]:
            trials = []
            for _ in range(args.runs):
                with source.open('rb') as input_file:
                    process = subprocess.Popen([str(binary), 'data', operation, '--pointer', '/k'],
                                               stdin=input_file, stdout=subprocess.PIPE,
                                               stderr=subprocess.PIPE, env=env)
                    try:
                        stdout, stderr = process.communicate(timeout=30)
                    except BaseException:
                        process.kill()
                        process.communicate()
                        raise
                    if process.returncode != 0 or stdout != expected or stderr:
                        raise RuntimeError(f'{operation} did not produce the exact expected result')
                    counters = Counters()
                    counters.cb = ctypes.sizeof(counters)
                    if not get_memory(int(process._handle), ctypes.byref(counters), ctypes.sizeof(counters)):
                        raise ctypes.WinError(ctypes.get_last_error())
                    if not counters.PeakWorkingSetSize or not counters.PeakPagefileUsage:
                        raise RuntimeError('Peak process counters unavailable')
                    trials.append({'peak_working_set_bytes': counters.PeakWorkingSetSize,
                                   'peak_committed_bytes': counters.PeakPagefileUsage})
            results.append({'operation': operation, 'records': 100000,
                            'distinct_keys': 100000 if unique else 1, 'input_bytes': len(data),
                            'output_bytes': len(expected), 'output_sha256': hashlib.sha256(expected).hexdigest(),
                            'trials': trials,
                            'median_peak_working_set_bytes': statistics.median(t['peak_working_set_bytes'] for t in trials),
                            'median_peak_committed_bytes': statistics.median(t['peak_committed_bytes'] for t in trials)})
    report = {'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'platform': platform.platform(), 'key_mode': args.keys, 'cases': results,
              'scope': 'Complete Windows CLI process OS peaks, not allocator-only measurements. '
                       'Includes startup, input and output buffering. Exact synthetic outputs checked; '
                       'sort is an unchanged control. No model/network requests or universal memory guarantee.'}
    rendered = json.dumps(report, indent=2) + '\n'
    if args.output:
        with args.output.open('x', encoding='utf-8') as destination:
            destination.write(rendered)
    else:
        print(rendered, end='')


if __name__ == '__main__':
    main()
