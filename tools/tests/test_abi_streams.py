"""The single-record and streaming decoder section (lock format 8)."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_streams', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_declarations import rust_streams
from abi_records import rust_records
from abi_i18n import rust_i18n
from abi_host_decode import rust_host_decode


class Streams(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((ROOT / 'abi/abi.def').read_text())

    def test_the_section_follows_the_records_and_needs_service_records(self):
        source = rust_streams(self.schema)
        for name in ('pub fn record_decode_prefix', 'pub fn records_decode_single',
                     'pub struct RecordStream', 'pub const RECORD_CARRY_MAX_BYTES: usize = 256;'):
            self.assertIn(name, source)
        generated = (ROOT / 'abi/generated/records_wire.rs').read_text()
        served = ABI.with_services(self.schema, ROOT / 'abi/services')
        self.assertEqual(generated, rust_records(self.schema) + source + rust_i18n(self.schema)
                         + rust_host_decode(served))
        without = copy.deepcopy(self.schema)
        without['records'] = [r for r in without['records']
                              if not r['name'].startswith('service.')]
        self.assertEqual(rust_streams(without), '')

    def test_the_lock_pins_the_section(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['lock_format'], 10)
        self.assertEqual(trusted['emitted_streams'], rust_streams(self.schema))
        ABI.check_lock(trusted, self.schema)
        original = ABI.rust_streams
        with patch.object(ABI, 'rust_streams', lambda s: original(s) + '// drift\n'):
            with self.assertRaisesRegex(ValueError, 'emitted streams contract'):
                ABI.check_lock(trusted, self.schema)
        legacy = copy.deepcopy(trusted)
        legacy['lock_format'] = 7
        del legacy['emitted_streams']
        del legacy['emitted_services']
        del legacy['emitted_host_decode']
        with patch.object(ABI, 'rust_streams', lambda s: original(s) + '// drift\n'):
            ABI.check_lock(legacy, self.schema)


if __name__ == '__main__':
    unittest.main()
