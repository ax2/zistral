"""Portable rules for the build-only Windows DLL gate (no SDK needed here)."""
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import windows_runtime as runtime


class RuntimeRules(unittest.TestCase):
    def test_reviewed_system_imports_are_accepted_case_insensitively(self):
        text = '\n'.join('    ' + name.upper() for name in runtime.SYSTEM_DLLS)
        self.assertEqual(runtime.checked_imports(text), sorted(runtime.SYSTEM_DLLS))

    def test_repeated_imports_and_localized_headings_do_not_change_the_set(self):
        text = '依赖项：\n    KERNEL32.dll\n    ws2_32.dll\n    kernel32.DLL\n  Summary\n'
        self.assertEqual(runtime.checked_imports(text), ['kernel32.dll', 'ws2_32.dll'])

    def test_redistributables_and_unreviewed_libraries_are_rejected(self):
        for name in ['VCRUNTIME140.dll', 'MSVCP140.dll', 'ucrtbased.dll',
                     'api-ms-win-crt-runtime-l1-1-0.dll', 'python312.dll', 'libssl-3-x64.dll', 'new-helper.dll', 'my runtime.dll', '额外运行库.dll', 'helper.exe', 'runtime-without-extension']:
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'unreviewed'):
                runtime.checked_imports('    KERNEL32.dll\n    ' + name + '\n')

    def test_empty_missing_core_and_non_import_dll_text_fail_closed(self):
        for text in ['', '    ws2_32.dll\n', 'Dump of file KERNEL32.dll\n', '    KERNEL32.dll extra\n']:
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, 'cannot establish'):
                runtime.checked_imports(text)

    def test_dumpbin_failure_cannot_be_hidden_by_success_shaped_stdout(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / 'candidate.exe';binary.write_bytes(b'fixture')
            with patch.object(runtime, 'find_dumpbin', return_value='dumpbin'), \
                 patch.object(runtime.subprocess, 'run', return_value=SimpleNamespace(returncode=1, stdout='    KERNEL32.dll\n')), \
                 self.assertRaisesRegex(RuntimeError, 'could not inspect'):
                runtime.verify(binary)

    def test_success_report_records_checked_binary_digest_and_no_user_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            binary = Path(temporary) / 'candidate.exe';binary.write_bytes(b'fixture')
            with patch.object(runtime, 'find_dumpbin', return_value='dumpbin'), \
                 patch.object(runtime.subprocess, 'run', return_value=SimpleNamespace(returncode=0, stdout='    KERNEL32.dll\n')) as invoke:
                report = runtime.verify(binary)
            self.assertTrue(report['ok'])
            self.assertEqual(report['kind'], 'windows-runtime-check')
            self.assertEqual(report['imports'], ['kernel32.dll'])
            self.assertEqual(report['binary_sha256'], runtime.hashlib.sha256(b'fixture').hexdigest())
            self.assertEqual(invoke.call_args.args[0], ['dumpbin', '/nologo', '/dependents', str(binary.resolve())])


if __name__ == '__main__':
    unittest.main()
