"""Plugin-defined stage schemas share the host's stage validators and emitters (#228)."""
import contextlib
import copy
import io
import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('stage_codec', ROOT / 'tools/stage-codec.py')
CLI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CLI)
from abi_stages import _validate_stage, load_stages, plugin_stages_source, source_stages


def host_stage():
    return json.loads((ROOT / 'abi/stages/core.status@1.def').read_text())


def plugin_stage(key='reader.status'):
    return dict(host_stage(), key=key)


def payload(key='reader.tap'):
    """One record of the host's status stage, carried as a payload: no subscribers, no reply."""
    stage = host_stage()
    return {'payload': key, 'major': 1, 'revision': stage['revision'], 'max_bytes': 3000,
            'records': [r for r in stage['records'] if r['name'] == 'status.ask']}


def module(source, name):
    start = source.index(f'pub mod {name} {{')
    end = source.find('\n/// Local records', start + 1)
    return source[start:end if end > 0 else len(source)]


def public_items(source):
    """The public items of a codec module, by kind and name."""
    import re
    return sorted(re.findall(r'pub (?:const fn|fn|const|struct|enum|static) (\w+)', source))


# One schema, the host's codec (tables) and a plugin's (steps): the same bytes both ways,
# the same refusals.
BOTH_CODECS = r'''
use host::core_status_v1 as h;
use plugin::reader_status_v1 as p;

fn both(items: &str, context: &[u8], item: &str, flags: u32, text: &str) {
    let (mut a, mut b) = ([0xAA_u8; 4096], [0xAA_u8; 4096]);
    let asked = (
        h::input_encode(&[h::Record::StatusAsk(h::StatusAsk::new(items, context))], &mut a),
        p::input_encode(&[p::Record::StatusAsk(p::StatusAsk::new(items, context))], &mut b),
    );
    assert_eq!(asked.0, asked.1);
    assert_eq!(a, b);
    if let Ok(n) = asked.0 {
        let (x, y) = (h::StatusAsk::decode(&b[..n]).unwrap(), p::StatusAsk::decode(&a[..n]).unwrap());
        assert_eq!((x.items, x.context), (y.items, y.context));
        assert_eq!((x.items, x.context), (items, context));
        for cut in 0..n {
            assert_eq!(h::input_decode(&a[..cut]).err(), p::input_decode(&a[..cut]).err());
        }
    }
    let (mut a, mut b) = ([0xAA_u8; 4096], [0xAA_u8; 4096]);
    let rows = [h::Record::StatusValue(h::StatusValue::new(item, flags, text)); 3];
    let own = [p::Record::StatusValue(p::StatusValue::new(item, flags, text)); 3];
    let replied = (h::reply_encode(&rows, &mut a), p::reply_encode(&own, &mut b));
    assert_eq!(replied.0, replied.1);
    assert_eq!(a, b);
    if let Ok(n) = replied.0 {
        let (x, y) = (h::reply_decode(&b[..n]).unwrap(), p::reply_decode(&a[..n]).unwrap());
        assert_eq!(format!("{:?}", x.collect::<Vec<_>>()), format!("{:?}", y.collect::<Vec<_>>()));
        // A bad byte anywhere is refused or read alike.
        for at in 0..n {
            let mut bad = a;
            bad[at] ^= 0x81;
            let show = |r: Result<Vec<String>, i32>| format!("{r:?}");
            assert_eq!(
                show(h::reply_decode(&bad[..n]).map(|s| s.map(|r| format!("{r:?}")).collect())),
                show(p::reply_decode(&bad[..n]).map(|s| s.map(|r| format!("{r:?}")).collect())),
            );
        }
    }
}

#[test]
fn the_same_bytes_and_the_same_refusals() {
    let long = "x".repeat(129);
    both("a\nb", &[1, 2, 3], "battery", 7, "87 %");
    both("clock", &[], "", 0, "");
    both("", &[9; 1024], "é", u32::MAX, "日本");
    both("a\n\nb", &[9; 1025], &long, 1, &long);
    both(&"item\n".repeat(8), &[], &long[..128], 0x80, &long[..128]);
}
'''


class PluginStages(unittest.TestCase):
    def test_a_plugin_stage_gets_the_host_codec_under_its_own_key(self):
        # The host's stages walk tables with one interpreter (#307); a plugin's own few
        # records keep their decode, length and write steps. Two texts, one codec: the
        # same public items, and (below) the same bytes.
        host = source_stages({'minor': 65535, 'stages': [host_stage()], 'records': []})
        plugin = plugin_stages_source([plugin_stage()])
        self.assertEqual(public_items(module(host, 'core_status_v1')),
                         public_items(module(plugin, 'reader_status_v1')))
        self.assertIn('STATUS_ASK_FIELDS', host)
        self.assertNotIn('_FIELDS', plugin)
        self.assertNotIn('unsafe', plugin)
        self.assertIn('pub static STAGES: &[StageSchema]', plugin)
        self.assertIn('reader_status_v1::KEY', plugin)

    @unittest.skipUnless(shutil.which('rustc'), 'rustc')
    def test_both_codecs_of_a_schema_write_and_read_the_same_bytes(self):
        host = source_stages({'minor': 65535, 'stages': [host_stage()], 'records': []})
        plugin = plugin_stages_source([plugin_stage()])
        with tempfile.TemporaryDirectory() as folder:
            source = Path(folder) / 'both.rs'
            source.write_text('#![allow(dead_code)]\nmod host {\n' + host + '}\nmod plugin {\n'
                              + plugin + '}\n' + BOTH_CODECS)
            binary = Path(folder) / 'both'
            subprocess.run(['rustc', '--edition', '2024', '--test', str(source), '-o',
                            str(binary)], check=True, capture_output=True)
            subprocess.run([str(binary)], check=True, capture_output=True)

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


class InputRows(unittest.TestCase):
    def test_a_plugin_stage_may_take_several_input_records(self):
        source = module(plugin_stages_source([dict(plugin_stage(), input_rows=4)]),
                        'reader_status_v1')
        for line in ('pub const INPUT_ROWS: usize = 4;', 'MAX_INPUT, INPUT_ROWS, INPUT_TAGS)',
                     'if records.is_empty() { return Err(-2); }', 'REPLY_ROWS.max(INPUT_ROWS)',
                     # The batch writer takes as many input rows as the codec.
                     'if self.rows >= INPUT_ROWS { return Err(-2); }'):
            self.assertIn(line, source)
        self.assertNotIn('MAX_INPUT, 1, INPUT_TAGS)', source)
        # Without the key, the codec is the one-record host form.
        self.assertNotIn('INPUT_ROWS', plugin_stages_source([plugin_stage()]))

    def test_input_rows_is_a_plugin_stage_s_and_bounded(self):
        for rows in (0, 1, 65, True, '4'):
            with self.subTest(rows=rows), self.assertRaises(ValueError):
                plugin_stages_source([dict(plugin_stage(), input_rows=rows)])
        with self.assertRaisesRegex(ValueError, 'stage schema keys'):
            _validate_stage(dict(host_stage(), input_rows=4), 65535)
        with self.assertRaisesRegex(ValueError, 'payload schema keys'):
            plugin_stages_source([dict(payload(), input_rows=4)])


class Payloads(unittest.TestCase):
    def test_a_payload_gets_a_codec_but_no_stage_row(self):
        source = plugin_stages_source([plugin_stage(), payload()])
        self.assertIn('pub mod reader_tap_v1 {', source)
        self.assertNotIn('reader_tap_v1::KEY', source)
        self.assertIn('reader_status_v1::KEY', source)
        tap = module(source, 'reader_tap_v1')
        # The reply side is sealed: no tags, no rows, cap 0.
        for line in ('pub const MAX_REPLY: usize = 0;', 'pub const REPLY_ROWS: usize = 0;',
                     'pub const REPLY_TAGS: &[u32] = &[];', 'pub const MAX_INPUT: usize = 3000;'):
            self.assertIn(line, tap)
        self.assertIn('pub mod reader_tap_v1 {', plugin_stages_source([payload()]))

    def test_a_payload_is_checked_as_a_stage_input(self):
        for name, value in (('max_bytes', 0), ('max_bytes', 3841), ('major', 0),
                            ('payload', 'core.tap'), ('records', [])):
            with self.subTest(name=name), self.assertRaises(ValueError):
                plugin_stages_source([dict(payload(), **{name: value})])
        with self.assertRaisesRegex(ValueError, 'exceeds cap'):
            plugin_stages_source([dict(payload(), max_bytes=2)])
        with self.assertRaisesRegex(ValueError, 'payload schema keys'):
            plugin_stages_source([dict(payload(), reply=[])])
        with self.assertRaisesRegex(ValueError, 'one per key'):
            plugin_stages_source([payload('reader.status'), plugin_stage()])


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
        (self.schemas / 'reader.tap@1.def').write_text(json.dumps(payload(), indent=2))
        (self.schemas / 'reader.style@1.def').write_text(
            json.dumps(dict(plugin_stage('reader.style'), input_rows=8), indent=2))
        self.assertEqual(CLI.main([str(self.schemas), str(self.out)]), 0)
        crate = self.dir / 'lib.rs'
        crate.write_text(f'#![no_std]\n#![deny(warnings)]\n#[path = {json.dumps(str(self.out))}]\n'
                         'pub mod wire;\n')
        built = subprocess.run(['rustc', '--edition', '2024', '--crate-type', 'lib',
                                '--out-dir', str(self.dir), str(crate)],
                               capture_output=True, text=True)
        self.assertEqual(built.returncode, 0, built.stderr)

    @unittest.skipUnless(shutil.which('rustfmt'), 'rustfmt')
    def test_plugin_owns_every_key(self):
        self.assertEqual(CLI.main([str(self.schemas), str(self.out), '--plugin', 'reader']), 0)
        for plugin in ('other', 'read', 'Reader', 'x' * 41, ''):
            with self.subTest(plugin=plugin):
                self.out.unlink(missing_ok=True)
                self.assertEqual(CLI.main([str(self.schemas), str(self.out), '--plugin', plugin]), 1)
                self.assertFalse(self.out.exists())

    def test_unreadable_schemas_are_one_line_and_exit_1(self):
        (self.schemas / 'reader.tap@1.def').mkdir()
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(CLI.main([str(self.schemas), str(self.out)]), 1)
        self.assertEqual(err.getvalue().count('\n'), 1, err.getvalue())
        self.assertNotIn('Traceback', err.getvalue())
        (self.schemas / 'reader.tap@1.def').rmdir()
        (self.schemas / 'reader.tap@1.def').write_bytes(b'\xff\xfe')
        with contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(CLI.main([str(self.schemas), str(self.out)]), 1)
        self.assertEqual(err.getvalue().count('\n'), 1, err.getvalue())
        self.assertFalse(self.out.exists())

    def test_a_failed_rustfmt_is_one_line_and_exit_1(self):
        self.out.write_text('kept\n')
        failed = subprocess.CalledProcessError(
            1, ['rustfmt'], stderr='error: expected item\n --> <stdin>:3:1\n  |\n3 | }\n')
        for flags in ([], ['--check']):
            with self.subTest(flags=flags), mock.patch.object(CLI, 'rustfmt', side_effect=failed), \
                    contextlib.redirect_stderr(io.StringIO()) as err:
                self.assertEqual(CLI.main([str(self.schemas), str(self.out)] + flags), 1)
            self.assertEqual(err.getvalue().count('\n'), 1, err.getvalue())
            self.assertTrue(err.getvalue().startswith('stage-codec: rustfmt failed: error:'))
            self.assertIn('<stdin>:3:1', err.getvalue())
            self.assertEqual(self.out.read_text(), 'kept\n')

    def test_an_unwritable_output_is_one_line_and_exit_1(self):
        out = self.dir / 'missing' / 'wire.rs'
        with mock.patch.object(CLI, 'rustfmt', side_effect=lambda source: source), \
                contextlib.redirect_stderr(io.StringIO()) as err:
            self.assertEqual(CLI.main([str(self.schemas), str(out)]), 1)
        self.assertEqual(err.getvalue().count('\n'), 1, err.getvalue())
        self.assertTrue(err.getvalue().startswith(f'stage-codec: cannot write {out}:'))
        self.assertFalse(out.parent.exists())

    def test_load_stages_names_its_file(self):
        (self.schemas / 'reader.status@1.def').rename(self.schemas / 'reader.other@1.def')
        with self.assertRaisesRegex(ValueError, 'own key and major'):
            load_stages(self.schemas)


if __name__ == '__main__':
    unittest.main()
