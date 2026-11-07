"""The host decoder section (lock format 10, #985)."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_host_decode', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_host_decode import host_sent, rust_host_decode


class HostDecode(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.with_services(json.loads((ROOT / 'abi/abi.def').read_text()),
                                        ROOT / 'abi/services')

    def test_only_what_plugins_send_is_decoded(self):
        source = rust_host_decode(self.schema)
        for name in ('fn host_decode<', 'pub fn host_records_decode_into',
                     'pub fn host_decode_prefix', 'pub fn host_decode_one',
                     'pub const EMPTY: Self'):
            self.assertIn(name, source)
        arms = source[source.index('fn host_decode<'):source.index('impl RecordBatch')]
        sent = {r['name'] for r in self.schema['records'] if host_sent(self.schema, r['name'])}
        self.assertTrue({'event.started', 'input.key', 'draw.frame', 'stage.call',
                         'service.usage', 'service.plugin'} <= sent)
        for name in ('setting.spec', 'menu.entry', 'i18n.arg_str', 'stage.reply',
                     'registration.stage', 'service.echo'):
            self.assertFalse(host_sent(self.schema, name), name)
        self.assertIn('EVENT_STARTED_TAG\n', arms)
        self.assertNotIn('EventStarted::decode', arms)
        self.assertIn('SettingSpec::decode', arms)
        self.assertIn('ServiceEcho::decode', arms)

    def test_a_service_input_is_decoded_only_while_a_schema_takes_it(self):
        bare = copy.deepcopy(self.schema)
        bare.pop('services', None)
        self.assertTrue(host_sent(bare, 'service.echo'))

    def test_the_section_is_in_every_copy(self):
        source = rust_host_decode(self.schema)
        for path in ['abi/generated/records_wire.rs',
                     'crates/paperpaw-core/src/abi7/records_wire.rs',
                     'crates/paperpaw-sdk/src/native/declare/records_wire.rs']:
            self.assertTrue((ROOT / path).read_text().endswith(source), path)

    def test_the_lock_pins_the_section(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['lock_format'], 10)
        self.assertEqual(trusted['emitted_host_decode'], rust_host_decode(self.schema))
        ABI.check_lock(trusted, self.schema)
        original = ABI.rust_host_decode
        with patch.object(ABI, 'rust_host_decode', lambda s: original(s) + '// drift\n'):
            with self.assertRaisesRegex(ValueError, 'emitted host decoder contract'):
                ABI.check_lock(trusted, self.schema)
        legacy = copy.deepcopy(trusted)
        legacy['lock_format'] = 9
        del legacy['emitted_host_decode']
        with patch.object(ABI, 'rust_host_decode', lambda s: original(s) + '// drift\n'):
            ABI.check_lock(legacy, self.schema)


if __name__ == '__main__':
    unittest.main()
