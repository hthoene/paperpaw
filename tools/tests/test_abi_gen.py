"""Independent mutations of the M1 schema and its prior compatibility lock."""
import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

PATH = Path(__file__).resolve().parents[1] / 'abi-gen.py'
SPEC = importlib.util.spec_from_file_location('abi_gen', PATH)
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)


class SchemaTests(unittest.TestCase):
    def setUp(self):
        self.base = ABI.load(PATH.parents[1] / 'abi' / 'abi.def')

    def reject(self, candidate):
        with self.assertRaises(ValueError):
            ABI.compatible(self.base, candidate)

    def test_packaged_copy_is_generated_and_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            (root / 'abi/abi.def').write_text(ABI.canonical(self.base))
            ABI.run(['--root', str(root), '--write-lock'])
            packaged = root / 'crates/paperpaw-native/src/host_glue/paperpaw_abi.rs'
            self.assertEqual(packaged.read_bytes(),
                             (root / 'abi/generated/paperpaw_abi.rs').read_bytes())
            packaged.write_text('// stale packaged copy')
            with self.assertRaisesRegex(ValueError, 'stale generated file'):
                ABI.run(['--root', str(root), '--check'])
            ABI.run(['--root', str(root)])
            ABI.run(['--root', str(root), '--check'])

    def test_identity_and_canonical_object_order(self):
        ABI.compatible(self.base, copy.deepcopy(self.base))
        reordered = dict(reversed(list(self.base.items())))
        self.assertEqual(ABI.canonical(self.base), ABI.canonical(reordered))
        self.assertEqual(len(self.base['host_entries']), 42)

    def test_each_existing_contract_is_frozen(self):
        changes = {'id': 400, 'name': 'renamed', 'since': 2,
                   'result': 'length', 'effect': 'consume',
                   'ownership': 'release', 'reentry': 'visitor', 'max_bytes': 123}
        for key, value in changes.items():
            with self.subTest(key=key):
                new = copy.deepcopy(self.base)
                new['minor'] = 2
                new['host_entries'][0][key] = value
                self.reject(new)

    def test_slot_order_and_removal(self):
        new = copy.deepcopy(self.base)
        new['host_entries'][0:2] = reversed(new['host_entries'][0:2])
        self.reject(new)
        new = copy.deepcopy(self.base)
        new['host_entries'].pop()
        self.reject(new)

    def test_new_entry_requires_new_minor_and_fresh_identity(self):
        new = copy.deepcopy(self.base)
        item = copy.deepcopy(new['host_entries'][3])
        item.update(id=len(new['host_entries']), name='clock.new_query', since=2)
        new['host_entries'].append(item)
        self.reject(new)
        new['minor'] = 2
        ABI.compatible(self.base, new)
        item['id'] = 3
        self.reject(new)

    def test_argument_order_type_and_name_are_frozen(self):
        for mutation in ('type', 'name', 'order'):
            new = copy.deepcopy(self.base)
            args = new['host_entries'][0]['args']
            if mutation == 'order':
                args.reverse()
            else:
                args[0][mutation] = 'u32' if mutation == 'type' else 'other'
            self.reject(new)

    def test_versions_and_integer_types(self):
        for key in ('format', 'major', 'minor'):
            for value in (True, 1.0, -1, '1', None, 65536):
                with self.subTest(key=key, value=value):
                    new = copy.deepcopy(self.base)
                    new[key] = value
                    self.reject(new)
        for major, minor in ((8, 1), (7, 0), (7, 3)):
            new = dict(self.base, major=major, minor=minor)
            self.reject(new)

    def test_caps_and_unknown_keys(self):
        for value in (True, -1, 1.5, '512', 65537):
            new = copy.deepcopy(self.base)
            new['host_entries'][5]['max_bytes'] = value
            self.reject(new)
        new = copy.deepcopy(self.base)
        new['host_entries'][0]['owership'] = 'call'
        self.reject(new)

    def test_status_values_names_and_existing_meanings_are_frozen(self):
        for key, value in [('value', -99), ('name', 'ERR_OTHER'), ('since', 2)]:
            new = copy.deepcopy(self.base)
            new['minor'] = 2
            new['statuses'][1][key] = value
            self.reject(new)
        new = copy.deepcopy(self.base)
        new['statuses'].pop()
        self.reject(new)
        for item in [dict(name='ERR_BAD', value=True, since=1),
                     dict(name='ERR_BAD', value=0, since=1),
                     dict(name='ERR_BAD', value=-1, since=1),
                     dict(name='ERR_BAD', value=-14, since=True),
                     dict(name='bad', value=-14, since=1)]:
            new = copy.deepcopy(self.base)
            new['statuses'].append(item)
            with self.assertRaises(ValueError):
                ABI.validate(new)
        new = copy.deepcopy(self.base)
        new['statuses'].append(dict(name='ERR_NEXT', value=-14, since=1))
        self.reject(new)
        new['minor'] = 2
        new['statuses'][-1]['since'] = 2
        ABI.compatible(self.base, new)

    def test_mutation_must_not_use_length_retry(self):
        new = copy.deepcopy(self.base)
        new['host_entries'][15]['result'] = 'length'
        self.reject(new)

    def test_duplicate_json_keys_and_nonfinite_values(self):
        for contents in ('{"id":1,"id":2}', '{"id":1,"\\u0069d":2}',
                         '{"nested":{"a":1,"a":2}}', '{"n":NaN}',
                         '{"n":Infinity}', '{"n":-Infinity}'):
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'bad.json'
                path.write_text(contents)
                with self.assertRaises(ValueError):
                    ABI.load(path)

    def test_emitted_type_and_template_mutations_fail_trusted_baseline(self):
        trusted = ABI.lock_record(copy.deepcopy(self.base))
        with patch.dict(ABI.TYPES, {'handle': 'uint64_t'}):
            candidate = ABI.lock_record(copy.deepcopy(self.base))
            ABI.check_lock(candidate, self.base)
            with self.assertRaisesRegex(ValueError, 'emitted C contract'):
                ABI.check_lock(trusted, self.base)
        original = ABI.header
        for before, after in [('size_t len', 'uint32_t len'),
                              ('size_t cap', 'uint32_t cap'),
                              ('uint32_t size', 'uint64_t size'),
                              ('int32_t (*', 'int64_t (*')]:
            with self.subTest(template=before):
                with patch.object(ABI, 'header', lambda s: original(s).replace(before, after)):
                    with self.assertRaisesRegex(ValueError, 'emitted C contract'):
                        ABI.check_lock(trusted, self.base)

    def test_reserved_host_and_parameter_names(self):
        for reserved in ('size', 'major', 'minor', 'float', 'return', 'uint32_t'):
            new = copy.deepcopy(self.base)
            new['host_entries'][0]['name'] = reserved
            with self.assertRaises(ValueError):
                ABI.validate(new)
        for reserved in ('float', 'return', 'size_t', 'uint32_t', 'match', 'type', 'self'):
            new = copy.deepcopy(self.base)
            new['host_entries'][0]['args'][0]['name'] = reserved
            with self.assertRaises(ValueError):
                ABI.validate(new)

    def test_rust_template_changes_fail_trusted_baseline(self):
        trusted = ABI.lock_record(self.base)
        original = ABI.rust_module
        for before, after in [('pub len: usize', 'pub len: u32'),
                              ('pub cap: usize', 'pub cap: u32'),
                              ('pub size: u32', 'pub size: u64'),
                              ('-> i32', '-> i64'),
                              ('extern "C"', 'extern "Rust"')]:
            with self.subTest(template=before):
                with patch.object(ABI, 'rust_module', lambda s: original(s).replace(before, after)):
                    with self.assertRaisesRegex(ValueError, 'emitted Rust contract'):
                        ABI.check_lock(trusted, self.base)

    def test_c_only_draft_lock_preserves_its_contract_when_adding_rust(self):
        prior = ABI.lock_record(self.base, released_minor=1)
        prior['lock_format'] = 1
        del prior['emitted_rust']
        del prior['emitted_metadata']
        del prior['emitted_records']
        del prior['emitted_snapshot']
        del prior['emitted_color']
        del prior['emitted_commands']
        del prior['emitted_streams']
        del prior['emitted_services']
        del prior['emitted_host_decode']
        del prior['emitted_stages']
        ABI.check_lock(prior, self.base)
        with patch.dict(ABI.TYPES, {'handle': 'uint64_t'}):
            with self.assertRaisesRegex(ValueError, 'emitted C contract'):
                ABI.check_lock(prior, self.base)

    def test_multiple_additions_to_unreleased_minor(self):
        trusted = ABI.lock_record(copy.deepcopy(self.base), released_minor=0)
        candidate = copy.deepcopy(self.base)
        extra = copy.deepcopy(candidate['host_entries'][3])
        first = len(candidate['host_entries'])
        extra.update(id=first, name='clock.first', since=1)
        candidate['host_entries'].append(extra)
        ABI.check_lock(trusted, candidate)
        checkpoint = ABI.lock_record(copy.deepcopy(candidate), released_minor=0)
        candidate['host_entries'].append(dict(extra, id=first + 1, name='clock.second'))
        ABI.check_lock(checkpoint, candidate)
        released = ABI.lock_record(copy.deepcopy(self.base), released_minor=1)
        with self.assertRaises(ValueError):
            ABI.check_lock(released, candidate)

    def test_before_a_release_the_lock_only_matches_the_code(self):
        """One interface 7 until 1.0: an unreleased lock takes a breaking
        change when it is written again; a released one never does."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            definition = root / 'abi/abi.def'
            definition.write_text(ABI.canonical(self.base))
            ABI.run(['--root', str(root), '--write-lock'])
            trusted = root / 'trusted.lock'
            trusted.write_bytes((root / 'abi/abi.lock').read_bytes())
            # A breaking change of an unreleased callback signature.
            new = copy.deepcopy(self.base)
            draw = next(f for f in new['entry_contract']['tables'][0]['fields']
                        if f['name'] == 'draw')
            draw['callback']['args'].pop()
            definition.write_text(ABI.canonical(new))
            check = ['--root', str(root), '--baseline', str(trusted), '--check']
            with self.assertRaisesRegex(ValueError, 'lock stale'):
                ABI.run(check)
            ABI.run(['--root', str(root), '--write-lock'])
            self.assertEqual(ABI.run(check), 0)
            # Released: the trusted baseline refuses the same change.
            released = ABI.load(trusted)
            released['released_minor'] = released['schema']['minor']
            trusted.write_text(ABI.canonical(released))
            with self.assertRaisesRegex(ValueError, 'entry field'):
                ABI.run(check)

    def test_trusted_release_floor_survives_two_steps_in_all_modes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            definition = root / 'abi/abi.def'
            definition.write_text(ABI.canonical(self.base))
            ABI.run(['--root', str(root), '--write-lock'])
            trusted = root / 'trusted.lock'
            trusted.write_text(ABI.canonical(ABI.lock_record(self.base, 1)))
            lock = root / 'abi/abi.lock'
            before = lock.read_bytes()
            for mode in ([], ['--check'], ['--write-lock']):
                with self.subTest(mode=mode):
                    with self.assertRaisesRegex(ValueError, 'release boundary'):
                        ABI.run(['--root', str(root), '--baseline', str(trusted)] + mode)
                    self.assertEqual(lock.read_bytes(), before)
            # First legitimate transition: create a new candidate lock while
            # inheriting the released boundary from the trusted baseline.
            lock.unlink()
            ABI.run(['--root', str(root), '--baseline', str(trusted), '--write-lock'])
            self.assertEqual(ABI.load(lock)['released_minor'], 1)
            ABI.run(['--root', str(root), '--baseline', str(trusted), '--check'])
            # That candidate becomes the next integration baseline. A second
            # change must not sneak a since=1 entry into released minor 1.
            trusted.write_bytes(lock.read_bytes())
            new = copy.deepcopy(self.base)
            extra = dict(new['host_entries'][3], id=len(new['host_entries']), name='clock.late')
            new['host_entries'].append(extra)
            definition.write_text(ABI.canonical(new))
            for mode in ([], ['--check'], ['--write-lock']):
                with self.subTest(second_mode=mode):
                    with self.assertRaisesRegex(ValueError, 'backdated'):
                        ABI.run(['--root', str(root), '--baseline', str(trusted)] + mode)
            self.assertEqual(ABI.load(lock)['released_minor'], 1)

    def record(self):
        return {'id': 1, 'name': 'test.record', 'since': 1, 'fields': [
            {'name': 'value', 'type': 'u32', 'since': 1, 'optional': False,
             'ordinal': None, 'default': None, 'max_bytes': 0}]}

    def test_record_growth_is_per_record_not_global_prefix(self):
        self.base.pop('record_codec', None)
        self.base.pop('command_codec', None)
        self.base['records'] = [self.record(), dict(self.record(), id=2, name='test.second')]
        new = copy.deepcopy(self.base)
        new['minor'] = 2
        field = {'name': 'extra', 'type': 'u32', 'since': 2, 'optional': True,
                 'ordinal': 0, 'default': 0, 'max_bytes': 0}
        new['records'][0]['fields'].append(field)
        ABI.compatible(self.base, new)
        field['optional'] = False
        field['ordinal'] = field['default'] = None
        self.reject(new)

    def test_marking_a_field_secret_keeps_it_and_unmarking_breaks(self):
        # `secret` keeps a value out of Debug and changes no wire byte (#228,
        # draft 30): adding the mark is compatible, dropping it is not.
        record = next(r for r in self.base['records'] if r['name'] == 'event.text_typed')
        text = next(f for f in record['fields'] if f['name'] == 'text')
        marked = text.pop('secret', None)
        self.assertIs(marked, True)
        unmarked = copy.deepcopy(self.base)
        text['secret'] = True
        ABI.compatible(unmarked, self.base)
        self.reject(unmarked)
        # Any other change to the field still breaks, mark or not.
        resized = copy.deepcopy(self.base)
        next(f for f in next(r for r in resized['records'] if r['name'] == 'event.text_typed')
             ['fields'] if f['name'] == 'text')['max_bytes'] = 512
        self.reject(resized)

    def test_required_fields_and_defaults_cannot_change(self):
        self.base.pop('record_codec', None)
        self.base.pop('command_codec', None)
        self.base['records'] = [self.record()]
        new = copy.deepcopy(self.base)
        new['records'][0]['fields'].clear()
        self.reject(new)
        def appended(default):
            new = copy.deepcopy(self.base)
            new['minor'] = 2
            new['records'][0]['fields'].append({
                'name': 'opt', 'type': 'u32', 'since': 2, 'optional': True,
                'ordinal': 0, 'default': default, 'max_bytes': 0})
            return new
        for default in (True, -1, 2**32, '0'):
            self.reject(appended(default))
        # No default: the field is an `Option` (present iff its bit is set).
        ABI.compatible(self.base, appended(None))

    def test_generation_check_and_invalid_input_preserve_outputs(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            definition = root / 'abi/abi.def'
            definition.write_text(json.dumps(self.base))
            ABI.run(['--root', str(root), '--write-lock'])
            ABI.run(['--root', str(root), '--check'])
            generated = root / 'abi/abi.json'
            before = generated.read_bytes()
            broken = copy.deepcopy(self.base)
            broken['host_entries'][0]['args'][0]['type'] = 'Vec'
            definition.write_text(json.dumps(broken))
            with self.assertRaises(ValueError):
                ABI.run(['--root', str(root)])
            self.assertEqual(generated.read_bytes(), before)
            definition.write_text(json.dumps(self.base))
            generated.write_text('stale')
            with self.assertRaises(ValueError):
                ABI.run(['--root', str(root), '--check'])


if __name__ == '__main__':
    unittest.main()
