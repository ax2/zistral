import hashlib
import importlib.util
from io import BytesIO
from pathlib import Path
import tarfile
import tempfile
import unittest
import subprocess
import sys
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('unix_installer', Path(__file__).with_name('test-install-unix.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PortableRules(unittest.TestCase):
    def archive(self, root, change=None):
        target, version = 'x86_64-unknown-linux-gnu', '0.33.0'
        name = f'zistral-v{version}-{target}'
        items = [(name, None, tarfile.DIRTYPE, 0o755)]
        items += [(name+'/'+p, b'fake-binary' if p=='zi' else b'document', tarfile.REGTYPE, 0o755 if p=='zi' else 0o644)
                  for p in ['zi','README.md','README.zh-CN.md','LICENSE']]
        if change:
            items = change(items)
        archive = root/'portable.tar.gz'
        with tarfile.open(archive, 'w:gz') as tar:
            for path, data, kind, mode in items:
                info = tarfile.TarInfo(path)
                info.type, info.mode = kind, mode
                if data and kind == tarfile.REGTYPE:
                    info.size = len(data)
                if kind == tarfile.SYMTYPE:
                    info.linkname = '/tmp/unrelated'
                tar.addfile(info, BytesIO(data) if data and kind == tarfile.REGTYPE else None)
        return archive,target,version

    def extract(self, root, change=None, digest=None):
        archive,target,version = self.archive(root,change)
        output = root/'verified';output.mkdir()
        return module.portable_binary(archive,output,target,version,
                                      digest or hashlib.sha256(b'fake-binary').hexdigest())

    def test_valid_archive_copies_only_verified_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);out=self.extract(root)
            self.assertEqual(out.read_bytes(),b'fake-binary')
            self.assertEqual([p.name for p in out.parent.iterdir()],['zi'])

    def test_unexpected_absolute_or_parent_members_are_rejected(self):
        for extra in ['/tmp/unrelated','../unrelated']:
            with self.subTest(extra=extra), tempfile.TemporaryDirectory() as temporary:
                root=Path(temporary)
                with self.assertRaises(AssertionError):
                    self.extract(root,lambda items:items+[(extra,b'bad',tarfile.REGTYPE,0o644)])
                self.assertEqual(list((root/'verified').iterdir()),[])

    def test_symlink_payload_is_rejected_before_copy(self):
        with tempfile.TemporaryDirectory() as temporary, self.assertRaises(AssertionError):
            self.extract(Path(temporary),lambda items:[(p,d,tarfile.SYMTYPE if p.endswith('/zi') else k,m) for p,d,k,m in items])

    def test_duplicate_member_and_missing_documents_are_rejected(self):
        for change in [lambda items:items+[items[1]],lambda items:items[:-1]]:
            with tempfile.TemporaryDirectory() as temporary, self.assertRaises(AssertionError):
                self.extract(Path(temporary),change)

    def test_digest_mismatch_and_nonexecutable_binary_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary, self.assertRaises(AssertionError):
            self.extract(Path(temporary),digest='0'*64)
        with tempfile.TemporaryDirectory() as temporary, self.assertRaises(AssertionError):
            self.extract(Path(temporary),lambda items:[(p,d,k,0o644 if p.endswith('/zi') else m) for p,d,k,m in items])

    def test_preexisting_executable_stops_before_receipt_lookup(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);installed=root/'zi';installed.write_bytes(b'KEEP')
            with patch.object(module,'receipt') as query, self.assertRaises(AssertionError):
                module.ensure_fresh(installed,'x86_64-unknown-linux-gnu',root/'config/zistral/config.toml')
            query.assert_not_called();self.assertEqual(installed.read_bytes(),b'KEEP')

    def test_existing_receipt_or_config_refuses_before_changes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);config=root/'config/zistral/config.toml'
            with patch.object(module,'receipt',return_value=SimpleNamespace(returncode=0)), self.assertRaises(AssertionError):
                module.ensure_fresh(root/'zi','x86_64-unknown-linux-gnu',config)
            self.assertFalse(config.parent.exists())
            config.parent.mkdir(parents=True);config.write_bytes(b'KEEP_CONFIGURATION')
            with patch.object(module,'receipt',return_value=SimpleNamespace(returncode=1)), self.assertRaises(AssertionError):
                module.ensure_fresh(root/'zi','x86_64-unknown-linux-gnu',config)
            self.assertEqual(config.read_bytes(),b'KEEP_CONFIGURATION')

    def test_optimized_python_refuses_before_installer_commands(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);binary=root/'unused';binary.write_bytes(b'NOT_EXECUTABLE')
            result=subprocess.run([sys.executable,'-B','-O',str(Path(module.__file__)),
                '--binary',str(binary),'--dist',str(root),'--target','x86_64-unknown-linux-gnu'],capture_output=True)
            self.assertNotEqual(result.returncode,0)
            self.assertIn(b'without -O',result.stderr)
            self.assertEqual(binary.read_bytes(),b'NOT_EXECUTABLE')

    def test_bad_published_baseline_refuses_before_config_or_native_install(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);binary=root/'zi';binary.write_bytes(b'candidate')
            dist=root/'dist';dist.mkdir()
            for name in ['zistral-v0.33.0-linux-amd64.deb','zistral-v0.33.0-x86_64-unknown-linux-gnu.tar.gz']:
                (dist/name).write_bytes(b'unused')
            config_home=root/'config-home'
            with patch.object(module.sys,'platform','linux'), patch.dict(module.os.environ,{'XDG_CONFIG_HOME':str(config_home)}), patch.object(module,'ensure_fresh'), patch.object(module,'invoke',return_value=SimpleNamespace(stdout=b'zi 0.33.0')) as invoke:
                with self.assertRaises(ValueError):
                    module.verify(binary,dist,'x86_64-unknown-linux-gnu',root/'missing-baseline')
            self.assertEqual(invoke.call_count,1)  # Read candidate version only.
            self.assertFalse(config_home.exists())

    def test_wrong_platform_refuses_before_any_installer_command(self):
        with patch.object(module.sys,'platform','win32'), patch.object(module,'invoke') as invoke, self.assertRaises(AssertionError):
            module.verify(Path('unused'),Path('unused'),'x86_64-unknown-linux-gnu')
        invoke.assert_not_called()


if __name__=='__main__':
    unittest.main()
