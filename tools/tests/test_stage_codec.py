"""Plugin-defined stage schemas share the host's stage validators and emitters (#228)."""
import copy
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('stage_codec', ROOT / 'tools/stage-codec.py')
CLI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLI)
from abi_stages import _validate_stage, load_stages, plugin_stages_source, source_stages


def host_stage():
    return json.loads((ROOT / 'abi/stages/core.status@1.def').read_text())


def plugin_stage(key='reader.status'):
    return dict(host_stage(), key=key)


def module(source, name):
    start = source.index(f'pub mod {name} {{')
    end = source.find('\n/// Local records', start + 1)
    return source[start:end if end > 0 else len(source)]


class PluginStages(unittest.TestCase):
    def test_a_plugin_stage_gets_the_host_codec_under_its_own_key(self):
        host = source_stages({'minor': 65535, 'stages': [host_stage()], 'records': []})
        plugin = plugin_stages_source([plugin_stage()])
        self.assertEqual(
            module(host, 'core_status_v1').replace('core_status_v1', 'M')
            .replace('"core.status"', 'K'),
            module(plugin, 'reader_status_v1').replace('reader_status_v1', 'M')
            .replace('"reader.status"', 'K'))
        self.assertIn('pub static STAGES: &[StageSchema]', plugin)
        self.assertIn('reader_status_v1::KEY', plugin)

    def test_core_keys_are_the_host_s_and_other_keys_a_plugin_s(self):
        with self.assertRaisesRegex(ValueError, 'not core'):
            plugin_stages_source([host_stage()])
        with self.assertRaisesRegex(ValueError, r'core\.\*'):
            _validate_stage(plugin_stage(), 65535)
        for key in ('reader', 'Reader.status', 'reader..status', 'r.' + 'x' * 127):
            with self.subTest(key=key), self.assertRaises(ValueError):
                plugin_stages_source([plugin_stage(key)])

    def test_a_plugin_stage_mirrors_no_service_row(self):
        stage = plugin_stage()
        stage['records'][0] = dict(stage['records'][0], mirrors='service.end')
        with self.assertRaisesRegex(ValueError, 'mirrors no row'):
            plugin_stages_source([stage])

    def test_the_host_bounds_hold_for_plugin_stages(self):
        for name, value in (('max_input', 3841), ('max_reply', 3841), ('reply_rows', 65),
                            ('max_subscribers', 33), ('since', 0), ('major', 0)):
            stage = plugin_stage()
            stage[name] = value
            with self.subTest(name=name), self.assertRaises(ValueError):
                plugin_stages_source([stage])
        stage = plugin_stage()
        stage['max_reply'] = 8
        with self.assertRaisesRegex(ValueError, 'exceeds cap'):
            plugin_stages_source([stage])

    def test_schemas_are_one_per_key_and_major_in_order(self):
        with self.assertRaises(ValueError):
            plugin_stages_source([plugin_stage('reader.b'), plugin_stage('reader.a')])
        with self.assertRaises(ValueError):
            plugin_stages_source([plugin_stage(), plugin_stage()])
        with self.assertRaises(ValueError):
            plugin_stages_source([])
        self.assertIn('reader_b_v1', plugin_stages_source(
            [plugin_stage('reader.a'), plugin_stage('reader.b')]))


class Cli(unittest.TestCase):
    def setUp(self):
        self.dir = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.dir)
        self.schemas = self.dir / 'schemas'
        self.schemas.mkdir()
        (self.schemas / 'reader.status@1.def').write_text(json.dumps(plugin_stage(), indent=2))
        self.out = self.dir / 'wire.rs'

    @unittest.skipUnless(shutil.which('rustfmt'), 'rustfmt')
    def test_check_refuses_missing_and_stale_output_and_writes_nothing(self):
        self.assertEqual(CLI.main([str(self.schemas), str(self.out), '--check']), 1)
        self.assertFalse(self.out.exists())
        self.assertEqual(CLI.main([str(self.schemas), str(self.out)]), 0)
        self.assertEqual(CLI.main([str(self.schemas), str(self.out), '--check']), 0)
        self.out.write_text(self.out.read_text() + '\n')
        stale = self.out.read_text()
        self.assertEqual(CLI.main([str(self.schemas), str(self.out), '--check']), 1)
        self.assertEqual(self.out.read_text(), stale)

    @unittest.skipUnless(shutil.which('rustfmt'), 'rustfmt')
    def test_a_bad_schema_is_one_line_and_exit_1(self):
        bad = copy.deepcopy(plugin_stage('core.mine'))
        (self.schemas / 'reader.status@1.def').unlink()
        (self.schemas / 'core.mine@1.def').write_text(json.dumps(bad))
        self.assertEqual(CLI.main([str(self.schemas), str(self.out)]), 1)
        self.assertFalse(self.out.exists())

    @unittest.skipUnless(shutil.which('rustc') and shutil.which('rustfmt'), 'rustc')
    def test_the_output_builds_as_no_std(self):
        self.assertEqual(CLI.main([str(self.schemas), str(self.out)]), 0)
        crate = self.dir / 'lib.rs'
        crate.write_text(f'#![no_std]\n#![deny(warnings)]\n#[path = {json.dumps(str(self.out))}]\n'
                         'pub mod wire;\n')
        built = subprocess.run(['rustc', '--edition', '2024', '--crate-type', 'lib',
                                '--out-dir', str(self.dir), str(crate)],
                               capture_output=True, text=True)
        self.assertEqual(built.returncode, 0, built.stderr)

    def test_load_stages_names_its_file(self):
        (self.schemas / 'reader.status@1.def').rename(self.schemas / 'reader.other@1.def')
        with self.assertRaisesRegex(ValueError, 'own key and major'):
            load_stages(self.schemas)


if __name__ == '__main__':
    unittest.main()
