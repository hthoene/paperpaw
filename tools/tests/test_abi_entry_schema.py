"""Direct contract mutations for the inactive raw entry declarations."""
import copy
import json
from pathlib import Path
import sys
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools'))
from abi_entry import c_entry, compatible_entry, rust_entry, validate_entry


class EntrySchemaTests(unittest.TestCase):
    def setUp(self):
        self.schema = json.loads((ROOT / 'abi/abi.def').read_text())

    def mutate(self, path, value):
        candidate = copy.deepcopy(self.schema)
        parent = candidate
        for key in path[:-1]:
            parent = parent[key]
        parent[path[-1]] = value
        return candidate

    def appended(self, **field):
        candidate = copy.deepcopy(self.schema)
        candidate['entry_contract']['tables'][0]['fields'].append(field)
        return candidate

    def test_absent_contract_is_source_compatible_and_emits_nothing(self):
        prior = copy.deepcopy(self.schema)
        del prior['entry_contract']
        validate_entry(prior)
        self.assertEqual(c_entry(prior), '')
        self.assertEqual(rust_entry(prior), '')
        compatible_entry(prior, self.schema, 1)
        with self.assertRaisesRegex(ValueError, 'backdated entry contract'):
            compatible_entry(prior, self.schema, 2)
        with self.assertRaisesRegex(ValueError, 'removed entry contract'):
            compatible_entry(self.schema, prior, 1)

    def test_exact_keys_at_every_level(self):
        paths = [[], ['locator'], ['tables', 0], ['tables', 0, 'fields', 0],
                 ['tables', 0, 'fields', 2], ['tables', 0, 'fields', 2, 'callback'],
                 ['tables', 0, 'fields', 2, 'callback', 'args', 0]]
        for path in paths:
            with self.subTest(path=path):
                candidate = copy.deepcopy(self.schema)
                node = candidate['entry_contract']
                for key in path:
                    node = node[key]
                node['extra'] = 1
                with self.assertRaisesRegex(ValueError, 'keys'):
                    validate_entry(candidate)

    def test_invalid_shapes_types_and_since_values_fail(self):
        prefix = ['entry_contract']
        table = prefix + ['tables', 0]
        field = table + ['fields', 0]
        callback = table + ['fields', 2, 'callback']
        mutations = [(prefix, None), (prefix + ['since'], True),
                     (prefix + ['since'], 0), (prefix + ['since'], 2),
                     (['minor'], True), (['minor'], 65536),
                     (prefix + ['tables'], {}), (table + ['fields'], {}),
                     (table + ['fields'], []), (table + ['since'], False),
                     (table + ['since'], 0), (field + ['since'], 2),
                     (field + ['since'], 1.0), (field + ['type'], []),
                     (field + ['type'], 'usize'), (callback + ['nullable'], 1),
                     (callback + ['result'], 'status'), (callback + ['result'], []),
                     (callback + ['args'], {}),
                     (callback + ['args', 0, 'type'], 'unknown')]
        for path, value in mutations:
            with self.subTest(path=path, value=value):
                with self.assertRaises(ValueError):
                    validate_entry(self.mutate(path, value))

    def test_since_cannot_precede_its_enclosing_contract_or_table(self):
        candidate = copy.deepcopy(self.schema)
        candidate['minor'] = 2
        contract = candidate['entry_contract']
        contract['since'] = 2
        with self.assertRaises(ValueError):
            validate_entry(candidate)
        for table in contract['tables']:
            table['since'] = 2
        with self.assertRaises(ValueError):
            validate_entry(candidate)
        for table in contract['tables']:
            for field in table['fields']:
                field['since'] = 2
        # The heap word cannot precede its contract either.
        with self.assertRaises(ValueError):
            validate_entry(candidate)
        contract['heap_word']['since'] = 2
        validate_entry(candidate)
        prior = copy.deepcopy(self.schema)
        del prior['entry_contract']
        compatible_entry(prior, candidate, 2)

    def test_generated_macros_cannot_be_appended_field_names(self):
        names = ['PP_OK', 'PP_ABI_MAJOR', 'PP_ABI_MINOR'] + [
            'PP_' + item['name'] for item in self.schema['statuses']
            if item['name'].startswith('ERR_')]
        for name in names:
            with self.subTest(name=name):
                with self.assertRaises(ValueError):
                    validate_entry(self.appended(name=name, type='u32', since=1))

    def test_names_symbols_and_closed_tables(self):
        base = ['entry_contract']
        paths = [base + ['entry_symbol'], base + ['locator', 'symbol'],
                 base + ['tables', 0, 'fields', 0, 'name'],
                 base + ['tables', 0, 'fields', 2, 'callback', 'args', 0, 'name']]
        for path in paths:
            for value in ['a.b', '9bad', 'é', 'fn', 'struct', '_Reserved', 'PpHost', '']:
                with self.subTest(path=path, value=value):
                    with self.assertRaises(ValueError):
                        validate_entry(self.mutate(path, value))
        for name in ['Other', 'PpPlugin']:
            with self.assertRaises(ValueError):
                validate_entry(self.mutate(base + ['tables', 1, 'name'], name))
        with self.assertRaises(ValueError):
            validate_entry(self.mutate(base + ['tables'],
                                      list(reversed(self.schema['entry_contract']['tables']))))
        for name in ['PAPERPAW_ENTRY']:
            with self.assertRaisesRegex(ValueError, 'duplicate'):
                validate_entry(self.mutate(base + ['locator', 'symbol'], name))

    def test_required_prefix_and_duplicate_fields_arguments(self):
        table = ['entry_contract', 'tables', 0]
        for path, value in [(table + ['fields', 0, 'type'], 'u16'),
                            (table + ['fields', 0, 'name'], 'other'),
                            (table + ['fields', 2, 'callback', 'nullable'], False)]:
            with self.subTest(path=path):
                with self.assertRaisesRegex(ValueError, 'prefix'):
                    validate_entry(self.mutate(path, value))
        candidate = self.appended(name='size', type='u32', since=1)
        with self.assertRaisesRegex(ValueError, 'duplicate entry field'):
            validate_entry(candidate)
        candidate = copy.deepcopy(self.schema)
        args = candidate['entry_contract']['tables'][0]['fields'][2]['callback']['args']
        args.append(copy.deepcopy(args[0]))
        with self.assertRaisesRegex(ValueError, 'duplicate entry callback argument'):
            validate_entry(candidate)

    def test_locator_and_metadata_export_semantics(self):
        for path, value in [(['entry_contract', 'locator', 'kind'], 'pointer'),
                            (['entry_contract', 'locator', 'length'], 'size'),
                            (['entry_contract', 'entry_symbol'], 'OTHER_ENTRY')]:
            with self.subTest(path=path):
                with self.assertRaises(ValueError):
                    validate_entry(self.mutate(path, value))
        for key, value in [('kind', 9), ('size_source', 'pointer_bytes')]:
            with self.assertRaisesRegex(ValueError, 'metadata exports'):
                validate_entry(self.mutate(['metadata', 'exports', 0, key], value))
        candidate = self.mutate(['entry_contract', 'locator', 'symbol'], 'OTHER_METADATA')
        validate_entry(candidate)
        with self.assertRaisesRegex(ValueError, 'locator or export'):
            compatible_entry(self.schema, candidate, 1)

    def test_existing_fields_are_immutable_and_appended_fields_need_minor_floor(self):
        compatible_entry(self.schema, copy.deepcopy(self.schema), 1)
        candidate = self.appended(name='extra', type='u32', since=1)
        compatible_entry(self.schema, candidate, 1)
        with self.assertRaisesRegex(ValueError, 'backdated appended entry field'):
            compatible_entry(self.schema, candidate, 2)
        candidate['minor'] = 2
        candidate['entry_contract']['tables'][0]['fields'][-1]['since'] = 2
        compatible_entry(self.schema, candidate, 2)
        prior = copy.deepcopy(candidate)
        for change in [dict(name='renamed'), dict(type='u16'), dict(since=1)]:
            changed = copy.deepcopy(candidate)
            changed['entry_contract']['tables'][0]['fields'][-1].update(change)
            with self.assertRaisesRegex(ValueError, 'changed or removed entry field'):
                compatible_entry(prior, changed, 2)
        changed = copy.deepcopy(candidate)
        changed['entry_contract']['tables'][0]['fields'].pop()
        with self.assertRaisesRegex(ValueError, 'changed or removed entry field'):
            compatible_entry(prior, changed, 2)

    def test_emission_has_exact_callback_types_symbols_and_docs(self):
        c, rust = c_entry(self.schema), rust_entry(self.schema)
        self.assertIn('typedef struct PpPlugin PpPlugin;', c)
        self.assertIn('typedef struct PpEntry PpEntry;', c)
        self.assertIn('int32_t (*make)(const PpHost * host, void * host_ctx, '
                      'const PpPlugin ** out);', c)
        self.assertIn('void (*drop)(void * state);', c)
        self.assertIn('pub make: Option<unsafe extern "C" fn(host: *const PpHost, '
                      'host_ctx: *mut core::ffi::c_void, out: *mut *const PpPlugin) -> i32>', rust)
        self.assertIn('pub drop: Option<unsafe extern "C" fn(state: *mut core::ffi::c_void) -> ()>', rust)
        self.assertIn('pub const METADATA_SYMBOL: &str = "PAPERPAW_METADATA";', rust)
        self.assertIn('#define PP_METADATA_SYMBOL "PAPERPAW_METADATA"', c)
        self.assertIn('pub const ENTRY_SYMBOL: &str = "PAPERPAW_ENTRY";', rust)
        self.assertNotIn('HOST_SYMBOL', rust)
        self.assertNotIn('PAPERPAW_HOST', c)
        self.assertNotIn('#![', rust)
        lines = rust.splitlines()
        for index, line in enumerate(lines):
            if line.lstrip().startswith('pub ') and 'struct ' not in line:
                self.assertTrue(lines[index - 1].lstrip().startswith('///'), line)
        self.assertEqual(c_entry(self.schema), c)
        self.assertEqual(rust_entry(self.schema), rust)

    def test_extra_fields_preserve_order_and_support_nonnullable_callbacks(self):
        candidate = self.appended(name='extra', callback=dict(
            args=[dict(name='code', type='i32')], result='void', nullable=False), since=1)
        validate_entry(candidate)
        compatible_entry(self.schema, candidate, 1)
        self.assertIn('pub extra: unsafe extern "C" fn(code: i32) -> (),', rust_entry(candidate))
        self.assertIn('void (*extra)(int32_t code);', c_entry(candidate))
        fields = candidate['entry_contract']['tables'][0]['fields']
        fields.append(dict(name='tail', type='bytes', since=1))
        prior = copy.deepcopy(candidate)
        fields[-2:] = reversed(fields[-2:])
        with self.assertRaisesRegex(ValueError, 'changed or removed entry field'):
            compatible_entry(prior, candidate, 1)
        candidate = self.appended(name='empty', callback=dict(args=[], result='i32', nullable=True),
                                  since=1)
        self.assertIn('int32_t (*empty)(void);', c_entry(candidate))
        self.assertIn('pub empty: Option<unsafe extern "C" fn() -> i32>,', rust_entry(candidate))

    def test_entry_cannot_grow_because_metadata_profiles_freeze_its_size(self):
        candidate = copy.deepcopy(self.schema)
        candidate['minor'] = 2
        candidate['entry_contract']['tables'][1]['fields'].append(
            dict(name='extra', type='u32', since=2))
        with self.assertRaisesRegex(ValueError, 'PpEntry fields are fixed'):
            validate_entry(candidate)
        with self.assertRaisesRegex(ValueError, 'PpEntry fields are fixed'):
            compatible_entry(self.schema, candidate, 2)


if __name__ == '__main__':
    unittest.main()
