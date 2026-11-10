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
from abi_stages import (compatible_stages, expand_mirrors, load_stages, rust_stages,
                        source_stages, stages_digest, validate_stages)


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
        # The repository's input filter, or a test schema's only stage.
        return next((s for s in schema['stages'] if s['key'] == 'core.input-filter'),
                    schema['stages'][0])

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
                         ['core.book-badge', 'core.format', 'core.input-filter', 'core.lookup',
                          'core.status', 'core.widget'])
        self.assertTrue(all(s['records'][0]['id'] == 1 for s in stages))
        source = rust_stages(self.schema)
        for module in ['core_book_badge_v1', 'core_format_v1', 'core_input_filter_v1',
                       'core_lookup_v1', 'core_status_v1', 'core_widget_v1']:
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

    def test_independent_row_and_byte_caps_at_exact_boundaries(self):
        schema, _ = revision_schemas(self.schema['minor'])
        stage = self.stage(schema)
        stage['reply_rows'] = 2
        stage['records'][1]['fields'] = [dict(name='bytes', type='bytes', since=0,
            optional=False, ordinal=None, default=None, max_bytes=3833)]
        validate_stages(schema)
        tests = r'''
#[cfg(test)]
mod tests {
    use super::core_test_stage_v1::*;
    #[test]
    fn rows_and_bytes_are_independent_and_no_output_is_truncated() {
        let mut out = [0xa5; 4096];
        let full = [7; 3829];
        let rows = [Record::SampleReply(SampleReply::new(&full)),
                    Record::SampleReply(SampleReply::new(&[]))];
        let len = reply_encode(&rows, &mut out).unwrap();
        assert_eq!(len, 3840);
        assert_eq!(reply_decode(&out[..len]).unwrap().len(), 2);
        assert!(out[3840..].iter().all(|b| *b == 0xa5));
        let over = [7; 3830];
        let rows = [Record::SampleReply(SampleReply::new(&over)),
                    Record::SampleReply(SampleReply::new(&[]))];
        out.fill(0xa5);
        assert_eq!(reply_encode(&rows, &mut out), Err(-3));
        assert!(out.iter().all(|b| *b == 0xa5));
        let three = [Record::SampleReply(SampleReply::new(&[])); 3];
        assert_eq!(reply_encode(&three, &mut out), Err(-2));
        assert!(out.iter().all(|b| *b == 0xa5));
        assert_eq!(reply_decode(&[3, 2, 2, 0, 0, 2, 2, 0, 0, 2, 2, 0, 0]).err(), Some(-2));
        let len = SampleReply::new(&over).encode(&mut out).unwrap();
        out[0] = 2;
        out[len..len + 4].copy_from_slice(&[2, 2, 0, 0]);
        assert_eq!(len + 4, 3841);
        assert_eq!(reply_decode(&out[..len + 4]).err(), Some(-2));
    }
}
'''
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            source = path / 'stage-caps.rs'
            # Keep the generated inner attribute first in the crate.
            source.write_text(rust_stages(schema) + tests)
            binary = path / 'stage-caps-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)

    def test_size_checks_name_stage_and_required_and_available_bytes(self):
        for cap in ['max_input', 'max_reply']:
            candidate = copy.deepcopy(self.schema)
            self.stage(candidate)[cap] = 1
            with self.assertRaisesRegex(ValueError, r'core\.input-filter.*[0-9].*1'):
                validate_stages(candidate)
        candidate = copy.deepcopy(self.schema)
        status = candidate['stages'][1]
        status['reply_rows'] = 64
        validate_stages(candidate)  # Row and byte caps bind independently.

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
                     lambda s: next(x for x in s['stages']
                                    if x['key'] == 'core.status').update(reply_rows=7),
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
        prior = dict(self.schema, stages=[s for s in self.schema['stages']
                                          if s['key'] != 'core.input-filter'])
        with self.assertRaises(ValueError):
            compatible_stages(prior, self.schema, self.schema['minor'] + 1)
        grown = copy.deepcopy(self.schema)
        self.stage(grown)['since'] = self.schema['minor'] + 1
        compatible_stages(prior, grown, self.schema['minor'] + 1)



def varint(value):
    out = bytearray()
    while True:
        byte, value = value & 0x7f, value >> 7
        out.append(byte | (0x80 if value else 0))
        if not value:
            return bytes(out)


class MirrorTests(unittest.TestCase):
    """A stage record that `mirrors` a service row holds exactly that row's fields."""

    def setUp(self):
        self.schema = ABI.with_stages(ABI.load(ROOT / 'abi/abi.def'), ROOT / 'abi/stages')
        validate_stages(self.schema)
        self.kernel = {r['name']: r for r in self.schema['records']}

    def format_stage(self, schema):
        return next(s for s in schema['stages'] if s['key'] == 'core.format')

    def test_mirrors_take_the_kernel_fields_and_emit_the_tag_table(self):
        stage = self.format_stage(self.schema)
        mirrored = [r for r in stage['records'] if 'mirrors' in r]
        self.assertEqual(len(mirrored), 21)
        for record in mirrored:
            source = self.kernel[record['mirrors']]
            self.assertEqual([f['name'] for f in record['fields']],
                             [f['name'] for f in source['fields']])
            self.assertTrue(all(f['since'] == record['since'] for f in record['fields']))
        source = rust_stages(self.schema)
        pairs = ', '.join(f"({r['id']}, {self.kernel[r['mirrors']]['id']})" for r in mirrored)
        flat = ' '.join(source.split())
        self.assertIn('pub const SERVICE_TAGS: &[(u32, u32)] = &[' + pairs, flat.replace('[ (', '[('))
        # Stages without mirrors emit no table, so their output is unchanged.
        status = source[source.index('pub mod core_status_v1'):]
        status = status[:status.index('\n}\n')]
        self.assertNotIn('SERVICE_TAGS', status)

    def test_a_mirror_of_a_missing_or_non_service_record_is_refused(self):
        for target in ['service.no_such', 'event.tick']:
            stages = [{'key': 'core.test-mirror', 'records': [
                {'id': 1, 'name': 'x.y', 'since': 0, 'mirrors': target}]}]
            with self.assertRaisesRegex(ValueError, 'mirrors an abi.def service record'):
                expand_mirrors(stages, self.schema['records'])

    def test_mirrored_fields_written_by_hand_must_match(self):
        schema = copy.deepcopy(self.schema)
        record = next(r for r in self.format_stage(schema)['records'] if 'mirrors' in r)
        record['fields'][0]['max_bytes'] += 1
        with self.assertRaisesRegex(ValueError, 'exactly the fields'):
            validate_stages(schema)

    def test_every_stage_module_has_both_batch_writers(self):
        source = source_stages(self.schema)
        modules = source.count('\npub mod ')
        self.assertGreater(modules, 0)
        for side in ('Reply', 'Input'):
            self.assertEqual(source.count(f"pub struct {side}Writer<'w>"), modules, side)
        # The one-byte count needs at most 127 rows a side.
        from abi_records import local_codec_source
        stage = copy.deepcopy(self.format_stage(copy.deepcopy(self.schema)))
        stage['reply_rows'] = 128
        with self.assertRaisesRegex(ValueError, 'at most 127 rows'):
            local_codec_source(stage)

    def test_reply_kinds_are_capped_at_32_and_input_kinds_at_8(self):
        schema = copy.deepcopy(self.schema)
        stage = self.format_stage(schema)
        self.assertEqual(len(stage['reply']), 22)
        extra = [dict(id=100 + i, name=f'fmt.extra{i}', since=0,
                      fields=[field(name='n', since=0, optional=False)]) for i in range(11)]
        stage['records'] += extra
        stage['reply'] += [r['name'] for r in extra]
        with self.assertRaisesRegex(ValueError, 'reply needs 1..=32'):
            validate_stages(schema)

    def grown_kernel(self):
        """abi.def's records after a later minor appended an optional field to doc_state."""
        records = copy.deepcopy(self.schema['records'])
        row = next(r for r in records if r['name'] == 'service.doc_state')
        row['fields'].append(dict(name='later', type='u32', since=self.schema['minor'] + 1,
                                  optional=True, ordinal=0, default=None, max_bytes=0))
        return records

    def raw_format_stage(self):
        stage = json.loads((ROOT / 'abi/stages/core.format@1.def').read_text())
        return stage, next(r for r in stage['records'] if r['name'] == 'fmt.state')

    def test_a_field_appended_to_a_mirrored_row_needs_its_stage_revision(self):
        stage, _ = self.raw_format_stage()
        with self.assertRaisesRegex(ValueError, 'newer than the stage schema'):
            expand_mirrors([stage], self.grown_kernel())

    def test_mirrored_records_take_the_same_compatibility_check(self):
        records = self.grown_kernel()
        new = copy.deepcopy(self.schema)
        new['records'] = records
        new['minor'] = self.schema['minor'] + 1
        stage, record = self.raw_format_stage()
        stage['revision'] = 1
        record['field_since'] = {'later': 1}
        new['stages'] = [s if s['key'] != 'core.format' else expand_mirrors([stage], records)[0]
                         for s in new['stages']]
        validate_stages(new)
        compatible_stages(self.schema, new, self.schema['minor'] + 1)
        grown = next(r for r in self.format_stage(new)['records'] if r['name'] == 'fmt.state')
        self.assertEqual(grown['fields'][-1]['since'], 1)
        # The stamp stays when the stage's revision grows again.
        stage['revision'] = 2
        again = expand_mirrors([stage], records)[0]
        self.assertEqual(next(r for r in again['records'] if r['name'] == 'fmt.state'
                              )['fields'][-1]['since'], 1)
        # Stamped at the old revision, it is a changed record, not an appended field.
        record['field_since'] = {'later': 0}
        stage['revision'] = 1
        new['stages'] = [s if s['key'] != 'core.format' else expand_mirrors([stage], records)[0]
                         for s in new['stages']]
        with self.assertRaisesRegex(ValueError, 'appended fields need next revision'):
            compatible_stages(self.schema, new, self.schema['minor'] + 1)
        # A mirrored field that changes is refused like any other.
        changed = copy.deepcopy(self.schema)
        row = next(r for r in self.format_stage(changed)['records'] if r['name'] == 'fmt.state')
        row['fields'][0]['max_bytes'] = 1
        with self.assertRaisesRegex(ValueError, 'changed record field'):
            compatible_stages(self.schema, changed, self.schema['minor'] + 1)
        record['mirrors'] = 'service.doc_meta'
        del record['field_since']
        with self.assertRaisesRegex(ValueError, 'changed record'):
            compatible_stages(self.schema, dict(self.schema, stages=[
                s if s['key'] != 'core.format' else expand_mirrors([stage], records)[0]
                for s in self.schema['stages']]), self.schema['minor'] + 1)

    def test_retagging_never_overflows_the_service_reply(self):
        # A local tag below 128 is one byte, a service tag 1024-1535 two: each mirrored row
        # grows by at most one byte, so a full stage reply stays within the service's 4,096.
        kernel = {r['name']: r['id'] for r in self.schema['records']}
        for stage in self.schema['stages']:
            growth = max((len(varint(kernel[r['mirrors']])) - len(varint(r['id']))
                          for r in stage['records'] if 'mirrors' in r), default=0)
            self.assertLessEqual(stage['max_reply'] + stage['reply_rows'] * growth, 4096,
                                 stage['key'])
            if stage['key'] == 'core.format':
                self.assertEqual((growth, stage['max_reply'] + stage['reply_rows'] * growth),
                                 (1, 3904))
        # The generator refuses a stage whose full reply would overflow once retagged.
        with mock.patch('abi_stages.MAX_CALL_BYTES', 3903):
            with self.assertRaisesRegex(ValueError, 'retagged reply'):
                validate_stages(self.schema)

if __name__ == '__main__':
    unittest.main()
