"""Closed settings descriptor, compatibility floors, and generated-copy checks."""
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_settings', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_requirements import required_minor
from abi_settings import compatible_settings, rust_settings, validate_settings


class SettingsSchemaTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def test_settings_host_requirement_includes_codec_revision(self):
        schema = copy.deepcopy(self.schema)
        schema['minor'] = 2
        schema['settings_value']['since'] = 2
        ABI.validate(schema)
        self.assertEqual(required_minor(schema, [dict(kind='host', name='settings.read')], 0), 2)
        self.assertEqual(required_minor(schema, [dict(kind='host', name='heap.alloc')], 0), 1)

    def test_absence_preserves_old_emission_and_fixtures(self):
        prior = copy.deepcopy(self.schema)
        del prior['settings_value']
        validate_settings(prior)
        self.assertEqual(rust_settings(prior), '')
        self.assertNotIn('SettingValueRef', ABI.rust_module(prior))
        compatible_settings(prior, self.schema, 1)
        with self.assertRaisesRegex(ValueError, 'backdated'):
            compatible_settings(prior, self.schema, 2)
        with self.assertRaisesRegex(ValueError, 'removed'):
            compatible_settings(self.schema, prior, 1)
        fixture = ABI.load(ROOT / 'abi/fixtures/metadata/schema.json')
        self.assertNotIn('settings_value', fixture)
        self.assertNotIn('settings_encode', ABI.rust_module(fixture))

    def test_closed_keys_at_every_level(self):
        for path in [[], ['header', 0], ['kinds', 0]]:
            for mutation in ('extra', 'missing'):
                with self.subTest(path=path, mutation=mutation):
                    candidate = copy.deepcopy(self.schema)
                    node = candidate['settings_value']
                    for key in path:
                        node = node[key]
                    if mutation == 'extra':
                        node['extra'] = 1
                    else:
                        del node[next(iter(node))]
                    with self.assertRaisesRegex(ValueError, 'keys'):
                        validate_settings(candidate)

    def test_invalid_types_shapes_and_encodings(self):
        mutations = [([], None), (['since'], True), (['since'], 0), (['since'], 2),
                     (['layout'], False), (['layout'], 2), (['byte_order'], 'big'),
                     (['max_bytes'], True), (['max_bytes'], 4097), (['max_bytes'], 19),
                     (['header'], {}), (['header'], []), (['header', 0, 'type'], 'u64'),
                     (['header', 0, 'name'], 'other'), (['kinds'], {}), (['kinds'], []),
                     (['kinds', 0, 'tag'], True), (['kinds', 1, 'tag'], 0),
                     (['kinds', 2, 'type'], 'u64'), (['kinds', 3, 'max_bytes'], 4096),
                     (['kinds', 3, 'name'], 'bytes')]
        for path, value in mutations:
            with self.subTest(path=path, value=value):
                candidate = copy.deepcopy(self.schema)
                parent = candidate
                full = ['settings_value'] + path
                for key in full[:-1]:
                    parent = parent[key]
                parent[full[-1]] = value
                with self.assertRaises(ValueError):
                    validate_settings(candidate)
        for value in (True, 0, 65536):
            with self.assertRaises(ValueError):
                validate_settings(dict(self.schema, minor=value))

    def test_status_values_and_since_must_support_descriptor(self):
        for mutation in ('missing', 'wrong', 'late'):
            candidate = copy.deepcopy(self.schema)
            if mutation == 'missing':
                del candidate['statuses']
            elif mutation == 'wrong':
                candidate['statuses'][1]['value'] = -100
            else:
                candidate['minor'] = 2
                candidate['statuses'][1]['since'] = 2
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                validate_settings(candidate)

    def test_limits_are_derived_but_existing_descriptors_are_frozen(self):
        smaller = copy.deepcopy(self.schema)
        smaller['settings_value']['max_bytes'] = 1024
        smaller['settings_value']['kinds'][3]['max_bytes'] = 1012
        validate_settings(smaller)
        source = rust_settings(smaller)
        self.assertIn('SETTINGS_MAX_BYTES: usize = 1024;', source)
        self.assertIn('SETTINGS_MAX_TEXT_BYTES: usize = 1012;', source)
        with self.assertRaisesRegex(ValueError, 'changed settings'):
            compatible_settings(self.schema, smaller, 1)
        later = copy.deepcopy(self.schema)
        later['minor'] = 2
        later['settings_value']['since'] = 2
        compatible_settings(dict(self.schema, settings_value=self.schema['settings_value']),
                            self.schema, 1)
        before = copy.deepcopy(self.schema)
        del before['settings_value']
        # Slot-15 command values depend on this contract and are removed with it.
        del before['command_codec']
        before['records'] = [item for item in before['records']
                             if item['name'] != 'command.set_setting']
        later['command_codec']['since'] = 2
        for record in later['records']:
            if record['name'] == 'command.set_setting':
                record['since'] = 2
                for field in record['fields']:
                    field['since'] = 2
        ABI.compatible(before, later, released_minor=1)
        with self.assertRaisesRegex(ValueError, 'backdated'):
            ABI.compatible(before, self.schema, released_minor=1)

    def test_trusted_lock_pins_codec_bodies_even_after_candidate_regeneration(self):
        trusted = ABI.lock_record(self.schema)
        original = ABI.rust_settings
        for before, after in [('slot.write(*byte)', 'slot.write(0)'),
                              ('return Err(-3)', 'return Err(-2)'),
                              ('SETTINGS_MAX_BYTES: usize = 4096',
                               'SETTINGS_MAX_BYTES: usize = 8192')]:
            with self.subTest(mutation=before):
                with patch.object(ABI, 'rust_settings',
                                  lambda schema: original(schema).replace(before, after)):
                    candidate = ABI.lock_record(self.schema)
                    ABI.check_lock(candidate, self.schema)
                    with self.assertRaisesRegex(ValueError, 'emitted Rust contract'):
                        ABI.check_lock(trusted, self.schema)

    def test_standalone_and_core_copies_are_identical_and_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            (root / 'abi/abi.def').write_text(ABI.canonical(self.schema))
            ABI.run(['--root', str(root), '--write-lock'])
            paths = [root / 'abi/generated/settings_value.rs',
                     root / 'crates/paperpaw-core/src/abi7/settings_wire.rs']
            self.assertEqual(paths[0].read_bytes(), paths[1].read_bytes())
            self.assertIn(paths[0].read_text(),
                          (root / 'abi/generated/paperpaw_abi.rs').read_text())
            for path in paths:
                path.write_text('// stale')
                with self.assertRaisesRegex(ValueError, 'stale generated file'):
                    ABI.run(['--root', str(root), '--check'])
                ABI.run(['--root', str(root)])
            ABI.run(['--root', str(root), '--check'])


if __name__ == '__main__':
    unittest.main()
