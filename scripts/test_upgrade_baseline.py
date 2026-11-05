import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

import upgrade_baseline as module


class BaselineRules(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.data = b'PACKAGE'
        self.name = 'zistral-v0.33.0-test.deb'
        self.manifest = {'version':'0.33.0','tag':'v0.33.0','source_commit':'7'*40,
            'targets':{'test':{'installer':self.name,'bytes':len(self.data),
            'sha256':hashlib.sha256(self.data).hexdigest(),'binary_sha256':'8'*64}}}
        self.addCleanup(patch.stopall)
        patch.object(module, 'BASELINE', self.manifest).start()

    def seed(self, data=None):
        path = self.root/self.name
        path.write_bytes(self.data if data is None else data)
        return path

    def test_offline_verification_is_readonly_and_returns_pinned_metadata(self):
        path=self.seed()
        with patch.object(module.subprocess, 'run') as run:
            report=module.verify(self.root,'test')
        run.assert_not_called()
        self.assertEqual(report['installer'],str(path.resolve()))
        self.assertEqual(report['binary_sha256'],'8'*64)
        self.assertEqual(report['tag'],'v0.33.0')
        self.assertEqual(path.read_bytes(),self.data)
        self.assertEqual(list(self.root.iterdir()),[path])

    def test_size_and_digest_mismatch_refuse_without_modifying_files(self):
        for data in [b'OTHER', b'TAMPER!']:
            path=self.seed(data)
            with self.assertRaisesRegex(ValueError,'mismatch'), patch.object(module.subprocess,'run') as run:
                module.verify(self.root,'test')
            run.assert_not_called()
            self.assertEqual(path.read_bytes(),data)

    def test_missing_directory_and_nonregular_asset_are_preserved(self):
        missing=self.root/'missing'
        with self.assertRaises(ValueError): module.verify(missing,'test')
        self.assertFalse(missing.exists())
        asset=self.root/self.name;asset.mkdir()
        with self.assertRaises(ValueError): module.verify(self.root,'test')
        self.assertTrue(asset.is_dir())
        if os.name!='nt':
            asset.rmdir();os.symlink(self.root/'unrelated',asset)
            with self.assertRaises(ValueError): module.verify(self.root,'test')
            self.assertTrue(asset.is_symlink())

    def test_download_never_clobbers_an_existing_asset_directory(self):
        path=self.seed()
        with self.assertRaises(FileExistsError), patch.object(module.subprocess,'run') as run:
            module.verify(self.root,'test',download=True)
        run.assert_not_called();self.assertEqual(path.read_bytes(),self.data)

    def test_explicit_download_uses_exact_public_release_and_validates_bytes(self):
        destination=self.root/'download'
        def download(args,**kwargs):
            self.assertEqual(args,['gh','release','download','v0.33.0','--repo','ax2/zistral','--pattern',self.name,'--dir',str(destination)])
            self.assertEqual(kwargs,{'capture_output':True,'timeout':60})
            (destination/self.name).write_bytes(self.data)
            return SimpleNamespace(returncode=0)
        with patch.object(module.subprocess,'run',side_effect=download) as run:
            result=module.verify(destination,'test',download=True)
        self.assertEqual(result['installer_sha256'],hashlib.sha256(self.data).hexdigest())
        self.assertEqual(run.call_count,1)

    def test_download_failure_is_bounded_and_does_not_echo_auth_output(self):
        for error in [OSError('PRIVATE_TOKEN_993'),subprocess.TimeoutExpired('gh',60)]:
            destination=self.root/type(error).__name__
            with patch.object(module.subprocess,'run',side_effect=error), self.assertRaises(RuntimeError) as caught:
                module.verify(destination,'test',download=True)
            self.assertNotIn('PRIVATE_TOKEN_993',str(caught.exception))
            self.assertEqual(list(destination.iterdir()),[])

    def test_unknown_target_and_unsafe_filename_refuse_before_download(self):
        with patch.object(module.subprocess,'run') as run:
            with self.assertRaises(ValueError):module.verify(self.root/'missing','unknown',True)
            self.manifest['targets']['test']['installer']='../unrelated'
            with self.assertRaises(ValueError):module.verify(self.root/'missing','test',True)
        run.assert_not_called();self.assertFalse((self.root/'missing').exists())

    def test_optimized_python_refuses_before_creating_download_directory(self):
        destination=self.root/'download'
        result=subprocess.run([sys.executable,'-B','-O',str(Path(module.__file__)),
            '--assets',str(destination),'--target','x86_64-unknown-linux-gnu','--download'],capture_output=True)
        self.assertNotEqual(result.returncode,0);self.assertIn(b'without -O',result.stderr)
        self.assertFalse(destination.exists())


if __name__=='__main__':
    unittest.main()
