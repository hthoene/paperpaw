"""Contract mutations for metadata schema and resolved offsets."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('metadata_generator', ROOT / 'tools/abi-gen.py')
GEN = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GEN)
from abi_layout import SCALARS


class MetadataSchemaTests(unittest.TestCase):
    def setUp(self):
        self.schema = GEN.load(ROOT / 'abi/abi.def')

    def test_offsets_are_resolved_and_in_trusted_lock(self):
        resolved = GEN.resolved_metadata(self.schema)
        self.assertEqual(resolved['prefix']['size'], 32)
        self.assertEqual([f['offset'] for f in resolved['prefix']['fields']],
                         [0, 4, 6, 8, 12, 14, 16, 18, 20, 24, 26, 28, 30])
        trusted = GEN.lock_record(self.schema)
        with patch.dict(SCALARS, {'u16': ('I', 4)}):
            with self.assertRaisesRegex(ValueError, 'emitted metadata'):
                GEN.check_lock(trusted, self.schema)

    def test_format_and_limits_are_frozen(self):
        for key, value in [('magic', 'OTHER'), ('byte_order', 'big'),
                           ('max_bytes', 4096), ('max_services', 17),
                           ('max_features', True), ('max_exports', 3)]:
            with self.subTest(key=key):
                candidate = copy.deepcopy(self.schema)
                candidate['metadata'][key] = value
                with self.assertRaises(ValueError):
                    GEN.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['metadata']['max_bytes'] = 1024
        with self.assertRaisesRegex(ValueError, 'changed metadata'):
            GEN.compatible(self.schema, candidate)

    def test_layout_reordering_and_unknown_fields_fail(self):
        candidate = copy.deepcopy(self.schema)
        candidate['metadata']['prefix'].reverse()
        with self.assertRaises(ValueError):
            GEN.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['metadata']['records']['export'][0]['padding'] = 0
        with self.assertRaises(ValueError):
            GEN.validate(candidate)

    def test_old_draft_lock_can_gain_metadata_but_not_in_released_minor(self):
        prior = copy.deepcopy(self.schema)
        del prior['metadata']
        GEN.check_lock(GEN.lock_record(prior, 0), self.schema)
        with self.assertRaisesRegex(ValueError, 'backdated metadata'):
            GEN.check_lock(GEN.lock_record(prior, 1), self.schema)

    def test_requirement_catalog_additions_obey_minor_floor(self):
        candidate = copy.deepcopy(self.schema)
        item = dict(key='test.feature', revision=1, since=1)
        candidate['metadata']['features'].append(item)
        with self.assertRaises(ValueError):
            GEN.check_lock(GEN.lock_record(self.schema, 1), candidate)
        candidate['minor'] = 2
        item['since'] = 2
        GEN.check_lock(GEN.lock_record(self.schema, 1), candidate)
        prior = copy.deepcopy(candidate)
        item['revision'] = 2
        with self.assertRaises(ValueError):
            GEN.check_lock(GEN.lock_record(prior), candidate)


if __name__ == '__main__':
    unittest.main()
