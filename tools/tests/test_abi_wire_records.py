"""Generic wire records: tag ranges, field rules and a synthetic round trip."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_wire', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_records import rust_records, validate_records


def field(name, kind, cap=0, optional=False, ordinal=None, default=None):
    return dict(name=name, type=kind, since=1, optional=optional, ordinal=ordinal,
                default=default, max_bytes=cap)


def synthetic(name='setting.synthetic', tag=11, fields=None):
    """One record with every wire type and four appended optional fields."""
    if fields is None:
        fields = [field('a', 'u32'), field('b', 'u64'), field('c', 'i32'),
                  field('d', 'str', 16), field('e', 'bytes', 8),
                  field('f', 'u32', optional=True, ordinal=0, default=7),
                  field('g', 'str', 4, optional=True, ordinal=1, default=''),
                  field('h', 'bytes', 4, optional=True, ordinal=2, default=''),
                  field('i', 'i32', optional=True, ordinal=3, default=-3),
                  field('j', 'u64', optional=True, ordinal=4, default=0),
                  field('k', 'u32', optional=True, ordinal=5, default=None)]
    return dict(id=tag, name=name, since=1, fields=fields)


RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;

    fn plain() -> SettingSynthetic<'static> {
        SettingSynthetic {
            a: 1, b: 2, c: -1, d: "hi", e: &[9], f: 7, g: "", h: &[], i: -3, j: 0, k: None,
        }
    }

    #[test]
    fn defaults_are_left_out() {
        let want = [1, 11, 22, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 255,
                    2, b'h', b'i', 1, 9];
        let mut out = [0; 64];
        let n = records_encode(&[Record::SettingSynthetic(plain())], &mut out).unwrap();
        assert_eq!(&out[..n], want);
        let batch = records_decode(&want).unwrap();
        assert_eq!(batch.iter().next(), Some(Record::SettingSynthetic(plain())));
    }

    #[test]
    fn optional_fields_round_trip_in_ordinal_order() {
        let mut full = plain();
        full.f = 8;
        full.g = "z";
        full.h = &[5, 6];
        full.i = -4;
        full.j = u64::MAX;
        // A field with no default: 0 is a value like any other.
        full.k = Some(0);
        let mut out = [0; 128];
        let n = records_encode(&[Record::SettingSynthetic(full)], &mut out).unwrap();
        assert_eq!(&out[..8], [1, 11, 22 + 1 + 4 + 2 + 3 + 4 + 8 + 4, 1, 0b111111, 1, 0, 0]);
        assert_eq!(n, 1 + 2 + (22 + 1 + 4 + 2 + 3 + 4 + 8 + 4));
        let batch = records_decode(&out[..n]).unwrap();
        assert_eq!(batch.iter().next(), Some(Record::SettingSynthetic(full)));
        let mut some = plain();
        some.g = "q";
        let n = records_encode(&[Record::SettingSynthetic(some)], &mut out).unwrap();
        assert_eq!(out[3..5], [1, 0b10]);
        assert_eq!(records_decode(&out[..n]).unwrap().iter().next(),
                   Some(Record::SettingSynthetic(some)));
    }

    #[test]
    fn a_field_without_a_default_is_present_exactly_when_some() {
        let mut out = [0; 128];
        for k in [None, Some(0), Some(5), Some(u32::MAX)] {
            let mut record = plain();
            record.k = k;
            let n = records_encode(&[Record::SettingSynthetic(record)], &mut out).unwrap();
            if k.is_some() {
                assert_eq!(out[3..5], [1, 0b100000], "{k:?}");
            } else {
                assert_eq!(out[3], 0, "{k:?}");
            }
            assert_eq!(n, 25 + if k.is_some() { 1 + 4 } else { 0 }, "{k:?}");
            assert_eq!(records_decode(&out[..n]).unwrap().iter().next(),
                       Some(Record::SettingSynthetic(record)), "{k:?}");
        }
        // Its presence bit set without the field (the last four bytes cut): refused.
        let mut record = plain();
        record.k = Some(5);
        let n = records_encode(&[Record::SettingSynthetic(record)], &mut out).unwrap();
        let mut cut = [0u8; 128];
        cut[..n - 4].copy_from_slice(&out[..n - 4]);
        cut[2] -= 4;
        assert_eq!(records_decode(&cut[..n - 4]).map(|_| ()), Err(-2));
    }

    #[test]
    fn caps_and_utf8_and_forward_bits() {
        let mut out = [0; 128];
        let mut bad = plain();
        bad.g = "12345";
        assert_eq!(records_encode(&[Record::SettingSynthetic(bad)], &mut out), Err(-3));
        let bytes = [1, 11, 26, 2, 0b10, 0x80, 1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                     0, 1, 9, 1, b'z', 7, 7];
        let mut want = plain();
        want.g = "z";
        want.d = "";
        want.c = 0;
        assert_eq!(records_decode(&bytes).unwrap().iter().next(),
                   Some(Record::SettingSynthetic(want)));
        let mut some = plain();
        some.g = "z";
        let n = records_encode(&[Record::SettingSynthetic(some)], &mut out).unwrap();
        assert_eq!(out[n - 1], b'z');
        out[n - 1] = 0xff;
        assert_eq!(records_decode(&out[..n]).map(|_| ()), Err(-5));
        // A presence bit set without its field (`g`, the last two bytes): refused.
        let n = records_encode(&[Record::SettingSynthetic(some)], &mut out).unwrap();
        let mut cut = [0u8; 128];
        cut[..n - 2].copy_from_slice(&out[..n - 2]);
        cut[2] -= 2;
        assert_eq!(records_decode(&cut[..n - 2]).map(|_| ()), Err(-2));
    }
}
'''


class WireRecordTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def with_record(self, record):
        candidate = copy.deepcopy(self.schema)
        candidate['records'].append(record)
        return candidate

    def test_a_declared_record_is_generated_without_python_code(self):
        source = rust_records(self.with_record(synthetic()))
        for text in ['pub const SETTING_SYNTHETIC_TAG: u32 = 11;',
                     "pub struct SettingSynthetic<'a> {",
                     'pub const SETTING_SYNTHETIC_D_MAX_BYTES: usize = 16;',
                     'pub a: u32,', 'pub b: u64,', 'pub c: i32,', "pub d: &'a str,",
                     "pub e: &'a [u8],", 'pub f: u32,', 'pub i: i32,',
                     'body.opt_u32(bits, 0x01, 7)?', 'body.opt_i32(bits, 0x08, -3)?',
                     'body.opt_string(bits, 0x02, SETTING_SYNTHETIC_G_MAX_BYTES)?',
                     'record.write_bitmap(out, &mut offset);',
                     'Record::SettingSynthetic(record) => record.bits(),']:
            self.assertIn(text, source)

    def test_without_optional_fields_the_codec_has_no_bitmap_code(self):
        plain = synthetic(fields=[field('a', 'u32'), field('d', 'str', 16)])
        schema = self.with_record(plain)
        # The shipped schema declares optional fields (event.page_turned); leave it out.
        schema['records'] = [r for r in schema['records']
                             if not any(f['optional'] for f in r['fields'])]
        source = rust_records(schema)
        self.assertNotIn('write_bitmap', source)
        self.assertNotIn('bitmap_bits', source)
        self.assertIn('write_varint(0, out, &mut offset);', source)

    def test_generated_source_is_rustfmt_stable(self):
        rustfmt = shutil.which('rustfmt') or str(Path.home() / '.cargo/bin/rustfmt')
        for schema in (self.schema, self.with_record(synthetic())):
            with tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'wire.rs'
                path.write_text(rust_records(schema))
                subprocess.run([rustfmt, '--edition=2024', '--check', str(path)], check=True)

    def test_standalone_round_trip_of_every_wire_type(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'wire.rs'
            source.write_text(rust_records(self.with_record(synthetic())) + RUST_TESTS)
            binary = folder / 'wire-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)

    def test_every_family_accepts_its_range_and_refuses_the_rest(self):
        ranges = {'setting.x': (5, 31), 'resource.x': (5, 31), 'action.x': (5, 31),
                  'menu.x': (5, 31), 'screen.x': (5, 31), 'icon.x': (5, 31), 'event.x': (32, 43),
                  'i18n.x': (44, 47), 'service.x': (48, 55), 'command.x': (56, 62)}
        small = [field('a', 'u32')]
        for name, (low, high) in ranges.items():
            # Tags 5-10 and 32-37 are taken by declared records.
            first = 38 if name == 'event.x' else max(low, 11)
            for tag in sorted({first, high}):
                with self.subTest(name=name, tag=tag):
                    validate_records(self.with_record(synthetic(name, tag, small)))
            for tag in (low - 1, high + 1, 0, 63, 64, 65536):
                with self.subTest(name=name, refused=tag), self.assertRaises(ValueError):
                    validate_records(self.with_record(synthetic(name, tag, small)))
        with self.assertRaisesRegex(ValueError, 'outside its family range 32-43'):
            validate_records(self.with_record(synthetic('event.x', 5, small)))

    def test_unknown_family_duplicate_and_moved_initial_tags_are_refused(self):
        small = [field('a', 'u32')]
        with self.assertRaisesRegex(ValueError, 'no tag family'):
            validate_records(self.with_record(synthetic('mystery.x', 12, small)))
        with self.assertRaisesRegex(ValueError, 'duplicate record codec tag 6'):
            validate_records(self.with_record(synthetic('setting.x', 6, small)))
        candidate = copy.deepcopy(self.schema)
        next(r for r in candidate['records'] if r['name'] == 'event.tick')['id'] = 32
        with self.assertRaisesRegex(ValueError, 'outside its family range 1-3'):
            validate_records(candidate)
        candidate = copy.deepcopy(self.schema)
        next(r for r in candidate['records'] if r['name'] == 'setting.spec')['id'] = 32
        with self.assertRaisesRegex(ValueError, 'outside its family range 5-31'):
            validate_records(candidate)

    def test_scalar_defaults_must_fit_their_type(self):
        for kind, bad in [('u32', -1), ('u32', 2**32), ('u64', 2**64), ('i32', 2**31),
                          ('i32', -2**31 - 1)]:
            fields = [field('a', 'u32'), field('b', kind, optional=True, ordinal=0, default=bad)]
            with self.subTest(kind=kind, default=bad), self.assertRaises(ValueError):
                validate_records(self.with_record(synthetic(fields=fields)))
        fields = [field('a', 'u32'), field('b', 'i32', optional=True, ordinal=0, default=-2**31)]
        validate_records(self.with_record(synthetic(fields=fields)))
        # No default: an `Option`, for every scalar type.
        for kind in ('u32', 'u64', 'i32'):
            fields = [field('a', 'u32'), field('b', kind, optional=True, ordinal=0, default=None)]
            validate_records(self.with_record(synthetic(fields=fields)))

    def test_field_rules_are_enforced(self):
        required = [field('a', 'u32')]
        opt = lambda n, o, kind='u32', d=1, cap=0: field(n, kind, cap, True, o, d)
        bad = {
            'required after optional': required + [opt('b', 0), field('c', 'u32')],
            'ordinal gap': required + [opt('b', 1)],
            'ordinal repeated': required + [opt('b', 0), opt('c', 0)],
            'nine optional': required + [opt(f'o{i}', i) for i in range(9)],
            'text default': required + [opt('b', 0, 'str', 'x', 4)],
            'scalar default is text': required + [opt('b', 0, 'u32', '')],
            'text default is null': required + [opt('b', 0, 'str', None, 4)],
            'keyword name': [field('type', 'u32')],
            'upper name': [field('Abc', 'u32')],
            'duplicate name': required + [field('a', 'u32')],
            'unknown type': [field('a', 'f32')],
            'text without cap': [field('a', 'str')],
            'scalar with cap': [field('a', 'u32', 4)],
            'cap above a batch': [field('a', 'bytes', 4097)],
            'no fields': [],
        }
        for label, fields in bad.items():
            with self.subTest(label), self.assertRaises(ValueError):
                validate_records(self.with_record(synthetic(fields=fields)))

    def test_colliding_generated_names_are_refused(self):
        for name in ('resource.font.face', 'setting.group'):
            with self.subTest(name), self.assertRaises(ValueError):
                validate_records(self.with_record(synthetic(name, 12, [field('a', 'u32')])))

    def test_record_names_must_be_plain_dotted_lowercase(self):
        for name in ('setting.Group', 'setting', 'setting.', 'setting.1x', 'setting.a-b'):
            with self.subTest(name), self.assertRaises(ValueError):
                validate_records(self.with_record(synthetic(name, 12, [field('a', 'u32')])))


if __name__ == '__main__':
    unittest.main()
