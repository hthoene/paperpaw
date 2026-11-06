"""The i18n formatting limits: a closed, immutable descriptor and its constants."""
import copy
import importlib.util
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_i18n', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_i18n import rust_i18n  # noqa: E402


class I18nLimitTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def test_declared_limits_and_their_constants(self):
        self.assertEqual(self.schema['i18n_limits'],
                         {'since': 1, 'max_key_bytes': 128, 'max_args_bytes': 1024,
                          'max_args_records': 8})
        source = rust_i18n(self.schema)
        for line in ['I18N_FORMAT_MAX_KEY_BYTES: usize = 128;',
                     'I18N_FORMAT_MAX_ARGUMENT_BYTES: usize = 1024;',
                     'I18N_FORMAT_MAX_RECORDS: usize = 8;',
                     'I18N_DIGITS_MAX_BYTES: usize = 4096;']:
            self.assertIn(line, source)
        # The records codec carries them, in every copy.
        for path in ['abi/generated/records_wire.rs',
                     'crates/paperpaw-core/src/abi7/records_wire.rs',
                     'crates/paperpaw-sdk/src/native/declare/records_wire.rs']:
            self.assertTrue((ROOT / path).read_text().endswith(source), path)
        # Without the descriptor nothing is emitted: older locks re-render alike.
        without = copy.deepcopy(self.schema)
        without.pop('i18n_limits')
        self.assertEqual(rust_i18n(without), '')
        ABI.validate(without)

    def test_descriptor_is_closed_and_bounded(self):
        good = self.schema['i18n_limits']
        for change in [{'extra': 1}, {'since': 0}, {'since': 2}, {'since': True},
                       {'max_key_bytes': 0}, {'max_key_bytes': 256},
                       {'max_args_bytes': 0}, {'max_args_bytes': 4097},
                       {'max_args_records': 0}, {'max_args_records': 9}]:
            candidate = copy.deepcopy(self.schema)
            candidate['i18n_limits'] = dict(good, **change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                ABI.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['i18n_limits'].pop('max_key_bytes')
        with self.assertRaises(ValueError):
            ABI.validate(candidate)

    def test_limits_are_immutable_once_declared(self):
        ABI.compatible(self.schema, copy.deepcopy(self.schema))
        for change in [{'max_args_bytes': 512}, None]:
            candidate = copy.deepcopy(self.schema)
            if change is None:
                candidate.pop('i18n_limits')
            else:
                candidate['i18n_limits'].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                ABI.compatible(self.schema, candidate)


if __name__ == '__main__':
    unittest.main()
