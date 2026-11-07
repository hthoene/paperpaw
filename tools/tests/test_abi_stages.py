"""Stage-local schema validation, wire bounds and compatible growth (#228)."""
import copy
import importlib.util
import hashlib
import json
from pathlib import Path
import tempfile
import shutil
import subprocess
import unittest
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_stages', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_stages import (compatible_stages, load_stages, rust_stages, source_stages,
                        stages_digest, validate_stages)


def field(name='extra', kind='u32', since=1, optional=True):
    return dict(name=name, type=kind, since=since, optional=optional,
                ordinal=0 if optional else None, default=0 if optional else None, max_bytes=0)


STAGE_REVISION_TESTS = r'''
#[path = "old.rs"]
mod old;
#[path = "new.rs"]
mod new;

#[cfg(test)]
mod tests {
    use super::{new::core_test_stage_v1 as new, old::core_test_stage_v1 as old};

    #[test]
    fn old_input_defaults_every_appended_optional_field() {
        let mut bytes = [0; 3840];
        let len = old::SampleInput::new(42).encode(&mut bytes).unwrap();
        let decoded = new::SampleInput::decode(&bytes[..len]).unwrap();
        assert_eq!(decoded.number, 42);
        assert_eq!((decoded.count, decoded.maybe), (7, None));
        assert_eq!((decoded.note, decoded.bytes), ("", &[][..]));
        assert_eq!((decoded.signed, decoded.wide), (-9, 10));
        assert_eq!((decoded.maybe_wide, decoded.maybe_signed), (None, None));
        let mut canonical = [0; 3840];
        let canonical_len = decoded.encode(&mut canonical).unwrap();
        assert_eq!(&canonical[..canonical_len], &bytes[..len]);
    }

    #[test]
    fn new_input_round_trips_all_optional_fields_and_old_reader_skips_them() {
        let value = new::SampleInput::new(
            42, u32::MAX, Some(0), "abcdefghijklmnop", &[0x55; 16],
            i32::MIN, u64::MAX, Some(0), Some(0),
        );
        let mut bytes = [0; 3840];
        let len = value.encode(&mut bytes).unwrap();
        assert_eq!(new::SampleInput::decode(&bytes[..len]), Ok(value));
        assert_eq!(old::SampleInput::decode(&bytes[..len]).unwrap().number, 42);
        assert_eq!(bytes[4], 0xff, "all eight optional bits are present");

        // Two bitmap bytes and an unknown appended field from a later revision.
        assert!(bytes[2] < 123, "this fixture uses one-byte frame lengths");
        let mut extended = [0; 3840];
        extended[..3].copy_from_slice(&[1, 1, bytes[2] + 5]);
        extended[3..6].copy_from_slice(&[2, bytes[4], 0x80]);
        extended[6..len + 1].copy_from_slice(&bytes[5..len]);
        extended[len + 1..len + 5].copy_from_slice(&999_u32.to_le_bytes());
        assert_eq!(new::SampleInput::decode(&extended[..len + 5]), Ok(value));
        assert_eq!(old::SampleInput::decode(&extended[..len + 5]).unwrap().number, 42);
    }

    #[test]
    fn optional_text_and_blob_caps_are_checked_on_both_wire_paths() {
        let note = "abcdefghijklmnopq";
        let cases = [
            new::SampleInput::new(42, 7, None, note, &[], -9, 10, None, None),
            new::SampleInput::new(42, 7, None, "", &[0; 17], -9, 10, None, None),
        ];
        for value in cases {
            let mut bytes = [0xa5; 3840];
            assert_eq!(value.encode(&mut bytes), Err(-2));
            assert!(bytes.iter().all(|byte| *byte == 0xa5));
        }
        for bits in [4, 8] {
            let mut bytes = [0; 27];
            bytes[..10].copy_from_slice(&[1, 1, 24, 1, bits, 42, 0, 0, 0, 17]);
            assert_eq!(new::SampleInput::decode(&bytes), Err(-2));
        }
    }

    #[test]
    fn malformed_bitmaps_and_bytes_after_the_batch_are_rejected() {
        let bad: &[&[u8]] = &[
            &[1, 1, 5, 2, 0, 0, 0, 0],
            &[1, 1, 6, 0x80, 0, 42, 0, 0, 0],
            &[1, 1, 5, 0, 42, 0, 0, 0, 0],
        ];
        for bytes in bad {
            assert_eq!(old::SampleInput::decode(bytes), Err(-2));
            assert_eq!(new::SampleInput::decode(bytes), Err(-2));
        }
        // A known optional presence bit still requires its complete value.
        assert_eq!(new::SampleInput::decode(&[1, 1, 6, 1, 2, 42, 0, 0, 0]), Err(-2));
    }
}
'''


def revision_schemas(minor):
    base = field(name='number', since=0, optional=False)
    stage = dict(key='core.test-stage', major=1, revision=0, since=minor, test_only=True,
                 max_subscribers=1, max_input=3840, max_reply=3840, reply_rows=1,
                 input=['sample.input'], reply=['sample.reply'], records=[
                     dict(id=1, name='sample.input', since=0, fields=[base]),
                     dict(id=2, name='sample.reply', since=0, fields=[copy.deepcopy(base)]),
                 ])
    old = dict(minor=minor, stages=[stage])
    new = copy.deepcopy(old)
    new['stages'][0]['revision'] = 1
    fields = new['stages'][0]['records'][0]['fields']
    for ordinal, (name, kind, default, cap) in enumerate([
            ('count', 'u32', 7, 0), ('maybe', 'u32', None, 0),
            ('note', 'str', '', 16), ('bytes', 'bytes', '', 16),
            ('signed', 'i32', -9, 0), ('wide', 'u64', 10, 0),
            ('maybe_wide', 'u64', None, 0), ('maybe_signed', 'i32', None, 0)]):
        fields.append(dict(name=name, type=kind, since=1, optional=True,
                           ordinal=ordinal, default=default, max_bytes=cap))
    return old, new


class StageTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.with_stages(ABI.load(ROOT / 'abi/abi.def'), ROOT / 'abi/stages')
        validate_stages(self.schema)

    def stage(self, schema):
        return schema['stages'][0]

    def test_lock_pins_stage_codec_and_accepts_format_ten(self):
        schema = ABI.with_services(self.schema, ROOT / 'abi/services')
        trusted = ABI.lock_record(schema)
        self.assertEqual(trusted['lock_format'], 11)
        self.assertEqual(trusted['emitted_stages'], stages_digest(schema))
        changed = copy.deepcopy(trusted)
        changed['emitted_stages'] += '// changed\n'
        with self.assertRaisesRegex(ValueError, 'emitted stages'):
            ABI.check_lock(changed, schema)
        old = copy.deepcopy(schema)
        del old['stages']
        legacy = ABI.lock_record(old)
        legacy['lock_format'] = 10
        del legacy['emitted_stages']
        ABI.check_lock(legacy, schema)

    def test_lock_digest_is_independent_of_rustfmt_rendering(self):
        expected = hashlib.sha256(source_stages(self.schema).encode()).hexdigest()
        self.assertEqual(stages_digest(self.schema), expected)
        with mock.patch('abi_stages.rust_stages', return_value='// different formatting\n'), \
                mock.patch('subprocess.run', side_effect=AssertionError('digest invoked rustfmt')):
            self.assertEqual(stages_digest(self.schema), expected)

    def test_optional_revision_growth_compiles_and_interoperates_without_std(self):
        old, new = revision_schemas(self.schema['minor'])
        validate_stages(old)
        validate_stages(new)
        compatible_stages(old, new, self.schema['minor'] + 1)
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / 'old.rs').write_text(rust_stages(old))
            (folder / 'new.rs').write_text(rust_stages(new))
            source = folder / 'stage-revisions.rs'
            source.write_text('#![no_std]\n' + STAGE_REVISION_TESTS)
            binary = folder / 'stage-revisions-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)

    def test_local_record_name_cannot_shadow_the_stream(self):
        schema = copy.deepcopy(self.schema)
        stage = self.stage(schema)
        stage['records'][0]['name'] = 'record.stream'
        stage['input'] = ['record.stream']
        with self.assertRaisesRegex(ValueError, 'collides'):
            validate_stages(schema)

    def test_repository_schemas_are_stage_local_and_copies_match(self):
        stages = self.schema['stages']
        self.assertEqual([s['key'] for s in stages],
                         ['core.input-filter', 'core.status', 'core.widget'])
        self.assertTrue(all(s['records'][0]['id'] == 1 for s in stages))
        source = rust_stages(self.schema)
        for module in ['core_input_filter_v1', 'core_status_v1', 'core_widget_v1']:
            self.assertIn(f'pub mod {module}', source)
        self.assertIn('pub static STAGES:', source)
        self.assertNotIn('unsafe', source)
        for path in ['abi/generated/stages_wire.rs',
                     'crates/paperpaw-core/src/abi7/stages_wire.rs',
                     'crates/paperpaw-sdk/src/native/wire/stages_wire.rs']:
            self.assertEqual((ROOT / path).read_text(), source)

    def test_file_names_and_sorted_loading(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            self.assertEqual(load_stages(root), [])
            for stage in reversed(self.schema['stages']):
                (root / f'{stage["key"]}@{stage["major"]}.def').write_text(json.dumps(stage))
            self.assertEqual(load_stages(root), self.schema['stages'])
            (root / 'core.wrong@1.def').write_text(json.dumps(self.schema['stages'][0]))
            with self.assertRaises(ValueError):
                load_stages(root)

    def test_stage_envelope_is_closed_and_bounded(self):
        mutations = [
            dict(key='Core.stage'), dict(key='plugin.stage'), dict(key='core.' + 'x' * 124),
            dict(key='core.bad..stage'), dict(key='core.bad_stage'),
            dict(major=0), dict(major=65536), dict(major=True),
            dict(revision=-1), dict(revision=65536), dict(revision=False),
            dict(since=0), dict(since=self.schema['minor'] + 1), dict(since=True),
            dict(test_only=1), dict(max_subscribers=0), dict(max_subscribers=33),
            dict(max_subscribers=True), dict(max_input=0), dict(max_input=3841),
            dict(max_input=True), dict(max_reply=0), dict(max_reply=3841),
            dict(reply_rows=0), dict(reply_rows=65), dict(reply_rows=False),
            dict(input=[]), dict(reply=[]), dict(input=['filter.input'] * 2),
            dict(input=['absent.record']), dict(reply=['filter.input']),
            dict(input=['filter.verdict'], reply=['filter.verdict']),
            dict(input='filter.input'), dict(reply=[1]), dict(extra=0), dict(records=[]),
        ]
        for update in mutations:
            with self.subTest(update=update):
                candidate = copy.deepcopy(self.schema)
                self.stage(candidate).update(update)
                with self.assertRaises(ValueError):
                    validate_stages(candidate)
        for key in self.stage(self.schema):
            with self.subTest(missing=key):
                candidate = copy.deepcopy(self.schema)
                del self.stage(candidate)[key]
                with self.assertRaises(ValueError):
                    validate_stages(candidate)

    def test_unique_schemas_and_rust_module_names(self):
        for extra in [self.schema['stages'][0],
                      dict(self.schema['stages'][0], key='core.input.filter')]:
            candidate = copy.deepcopy(self.schema)
            candidate['stages'].append(copy.deepcopy(extra))
            candidate['stages'].sort(key=lambda s: (s['key'], s['major']))
            with self.assertRaises(ValueError):
                validate_stages(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['stages'].reverse()
        with self.assertRaises(ValueError):
            validate_stages(candidate)

    def test_local_record_rules(self):
        for update in [dict(id=0), dict(id=512), dict(id=True), dict(id=2),
                       dict(name='filter.verdict'), dict(name='Filter.input'),
                       dict(name='filter'), dict(since=-1), dict(since=1),
                       dict(since=True), dict(extra=0)]:
            with self.subTest(update=update):
                candidate = copy.deepcopy(self.schema)
                self.stage(candidate)['records'][0].update(update)
                with self.assertRaises(ValueError):
                    validate_stages(candidate)
        candidate = copy.deepcopy(self.schema)
        self.stage(candidate)['records'].reverse()
        with self.assertRaises(ValueError):
            validate_stages(candidate)

    def test_field_validation_is_shared_with_kernel_records(self):
        mutations = [dict(type='bool'), dict(max_bytes=0), dict(max_bytes=4097),
                     dict(optional=1), dict(ordinal=0), dict(default='x'),
                     dict(since=1), dict(name='type'), dict(name='Bad'),
                     dict(extra=0), dict(list=dict(separator='--', max_items=8, item_max_bytes=8))]
        for update in mutations:
            with self.subTest(update=update):
                candidate = copy.deepcopy(self.schema)
                self.stage(candidate)['records'][0]['fields'][0].update(update)
                with self.assertRaises(ValueError):
                    validate_stages(candidate)
        candidate = copy.deepcopy(self.schema)
        fields = self.stage(candidate)['records'][0]['fields']
        fields.append(copy.deepcopy(fields[0]))
        with self.assertRaises(ValueError):
            validate_stages(candidate)

    def test_side_lists_have_at_most_eight_distinct_records(self):
        candidate = copy.deepcopy(self.schema)
        stage = self.stage(candidate)
        for index in range(3, 11):
            stage['records'].append(dict(id=index, name=f'filter.row_{index}', since=0,
                                        fields=[field(since=0, optional=False)]))
        stage['input'] += [r['name'] for r in stage['records'][2:]]
        with self.assertRaises(ValueError):
            validate_stages(candidate)

    def test_size_checks_name_stage_and_required_and_available_bytes(self):
        for cap in ['max_input', 'max_reply']:
            candidate = copy.deepcopy(self.schema)
            self.stage(candidate)[cap] = 1
            with self.assertRaisesRegex(ValueError, r'core\.input-filter.*[0-9].*1'):
                validate_stages(candidate)
        candidate = copy.deepcopy(self.schema)
        status = candidate['stages'][1]
        status['reply_rows'] = 64
        with self.assertRaisesRegex(ValueError, r'core\.status.*[0-9].*3840'):
            validate_stages(candidate)

    def test_compatible_growth_requires_new_revisions(self):
        grown = copy.deepcopy(self.schema)
        stage = self.stage(grown)
        stage['revision'] = 1
        stage['records'][0]['fields'].append(field())
        stage['records'].append(dict(id=3, name='filter.extra', since=1,
                                     fields=[field(since=1, optional=False)]))
        stage['input'].append('filter.extra')
        validate_stages(grown)
        compatible_stages(self.schema, grown, self.schema['minor'] + 1)
        for mutate in [lambda s: s['records'][-1].update(since=0),
                       lambda s: s['records'][0]['fields'][-1].update(since=0)]:
            candidate = copy.deepcopy(grown)
            mutate(self.stage(candidate))
            with self.assertRaises(ValueError):
                compatible_stages(self.schema, candidate, self.schema['minor'] + 1)

    def test_compatibility_refuses_removal_mutation_and_shrinking_caps(self):
        mutations = [lambda s: s['stages'].pop(0),
                     lambda s: self.stage(s).update(major=2),
                     lambda s: self.stage(s).update(test_only=True),
                     lambda s: self.stage(s).update(since=2),
                     lambda s: self.stage(s).update(max_subscribers=3),
                     lambda s: self.stage(s).update(max_input=3839),
                     lambda s: self.stage(s).update(max_reply=3839),
                     lambda s: s['stages'][1].update(reply_rows=7),
                     lambda s: self.stage(s)['records'].pop(),
                     lambda s: self.stage(s)['records'][0].update(id=3),
                     lambda s: self.stage(s)['records'][0].update(name='filter.other'),
                     lambda s: self.stage(s)['records'][0]['fields'][0].update(max_bytes=127),
                     lambda s: self.stage(s)['records'][0]['fields'].pop(),
                     lambda s: self.stage(s).update(input=['filter.verdict']),
                     lambda s: self.stage(s).update(reply=['filter.input'])]
        for mutate in mutations:
            candidate = copy.deepcopy(self.schema)
            mutate(candidate)
            with self.assertRaises(ValueError):
                compatible_stages(self.schema, candidate, self.schema['minor'] + 1)
        prior = copy.deepcopy(self.schema)
        self.stage(prior)['revision'] = 1
        with self.assertRaises(ValueError):
            compatible_stages(prior, self.schema, self.schema['minor'] + 1)

    def test_new_schema_must_not_be_backdated(self):
        prior = dict(self.schema, stages=self.schema['stages'][1:])
        with self.assertRaises(ValueError):
            compatible_stages(prior, self.schema, self.schema['minor'] + 1)
        grown = copy.deepcopy(self.schema)
        self.stage(grown)['since'] = self.schema['minor'] + 1
        compatible_stages(prior, grown, self.schema['minor'] + 1)


if __name__ == '__main__':
    unittest.main()
