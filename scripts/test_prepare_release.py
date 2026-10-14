import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('prepare_release',Path(__file__).with_name('prepare-release.py'));module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

class Preparation(unittest.TestCase):
 def test_rejects_dirty_checkout_without_downloading(self):
  with tempfile.TemporaryDirectory() as raw,patch.object(module.subprocess,'check_output',side_effect=['a'*40+'\n',' M README.md\n']),patch.object(module,'gh') as gh:
   with self.assertRaisesRegex(ValueError,'clean'):module.prepare('123',Path(raw)/'output')
   gh.assert_not_called();self.assertFalse((Path(raw)/'output').exists())
 def test_rejects_wrong_commit_without_downloading(self):
  run={'status':'completed','conclusion':'success','head_sha':'b'*40}
  with tempfile.TemporaryDirectory() as raw,patch.object(module.subprocess,'check_output',side_effect=['a'*40+'\n','']),patch.object(module,'gh',return_value=json.dumps(run)) as gh:
   with self.assertRaisesRegex(ValueError,'exact'):module.prepare('123',Path(raw)/'output')
   self.assertEqual(gh.call_count,1);self.assertFalse((Path(raw)/'output').exists())
 def test_requires_native_host_job_even_when_run_is_green(self):
  run={'status':'completed','conclusion':'success','head_sha':'a'*40,'path':'.github/workflows/ci.yml','head_repository':{'full_name':module.REPOSITORY}}
  with tempfile.TemporaryDirectory() as raw,patch.object(module.subprocess,'check_output',side_effect=['a'*40+'\n','']),patch.object(module,'gh',side_effect=[json.dumps(run),json.dumps({'jobs':[]})]):
   with self.assertRaisesRegex(ValueError,'native-skills'):module.prepare('123',Path(raw)/'output')
   self.assertFalse((Path(raw)/'output').exists())
