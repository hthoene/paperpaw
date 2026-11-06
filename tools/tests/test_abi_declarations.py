"""Declaration records 5-10: pinned schemas, caps, and standalone golden vectors."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_decls', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_records import rust_records, validate_records

# The shapes of tags 5-10, pinned here: (name, wire type, byte cap; 0 for scalars).
DECLS = {
    'setting.group': [('name', 'str', 96), ('title', 'str', 96), ('order', 'i32', 0),
                      ('page', 'str', 96), ('parent', 'str', 128)],
    'setting.spec': [('name', 'str', 96), ('kind', 'u32', 0), ('flags', 'u32', 0),
                     ('group', 'str', 128), ('title', 'str', 96), ('help', 'str', 96),
                     ('default', 'bytes', 512), ('min', 'i32', 0), ('max', 'i32', 0),
                     ('choices', 'str', 1024)],
    'setting.extra': [('name', 'str', 96), ('unit', 'str', 96), ('section', 'str', 96),
                      ('follows', 'str', 128), ('shown_id', 'str', 128),
                      ('shown_value', 'str', 128), ('editor_kind', 'u32', 0),
                      ('editor', 'str', 96), ('step', 'i32', 0), ('flags', 'u32', 0),
                      ('check', 'u32', 0)],
    'resource.hyphenation': [('language', 'str', 35), ('path', 'str', 100)],
    'resource.font_face': [('name', 'str', 96), ('path', 'str', 100),
                           ('family', 'str', 64), ('style', 'u32', 0),
                           ('weight', 'u32', 0), ('role', 'str', 96),
                           ('priority', 'i32', 0)],
    'resource.font_extension': [('name', 'str', 96), ('path', 'str', 100),
                                ('extends', 'str', 128)],
}

RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;

    fn all(bytes: &[u8]) -> Result<std::vec::Vec<Record<'_>>, i32> {
        records_decode(bytes).map(|batch| batch.iter().collect())
    }

    #[test]
    fn tag_8_has_language_and_path_only() {
        let bytes = [1, 8, 8, 0, 2, b'd', b'e', 3, b'a', b'.', b'p'];
        let expected = Record::ResourceHyphenation(ResourceHyphenation { language: "de", path: "a.p" });
        assert_eq!(all(&bytes).unwrap(), [expected]);
        let mut out = [0; 16];
        assert_eq!(records_encode(&[expected], &mut out), Ok(bytes.len()));
        assert_eq!(&out[..bytes.len()], bytes);
    }

    #[test]
    fn extension_golden_and_mixed_batch() {
        let record = Record::ResourceFontExtension(ResourceFontExtension {
            name: "n", path: "p", extends: "e.f" });
        let mut out = [0; 32];
        let n = records_encode(&[record, Record::Registration(Registration::Draw { token: 1 })], &mut out).unwrap();
        assert_eq!(&out[..n], [2, 10, 9, 0, 1, b'n', 1, b'p', 3, b'e', b'.', b'f',
                               2, 5, 0, 1, 0, 0, 0]);
        assert_eq!(all(&out[..n]).unwrap().len(), 2);
    }

    #[test]
    fn caps_are_exact_for_text_and_bytes() {
        fn spec(default: &[u8]) -> Record<'_> {
            Record::SettingSpec(SettingSpec {
                name: "a", kind: 0, flags: 0, group: "g", title: "t", help: "", default,
                min: 0, max: 0, choices: "" })
        }
        let mut out = [0; 1024];
        assert!(records_encode(&[spec(&[0; 512])], &mut out).is_ok());
        assert_eq!(records_encode(&[spec(&[0; 513])], &mut out), Err(-3));
        let long = "x".repeat(35);
        let ok = Record::ResourceHyphenation(ResourceHyphenation { language: &long, path: "p" });
        assert!(records_encode(&[ok], &mut out).is_ok());
        let long = "x".repeat(36);
        let bad = Record::ResourceHyphenation(ResourceHyphenation { language: &long, path: "p" });
        assert_eq!(records_encode(&[bad], &mut out), Err(-3));
    }

    #[test]
    fn invalid_utf8_and_unknown_tags_are_distinct_statuses() {
        assert_eq!(all(&[1, 8, 5, 0, 1, 0xff, 1, b'p']).map(|_| ()), Err(-5));
        assert_eq!(all(&[1, 11, 1, 0]).map(|_| ()), Err(-1));
    }
}
'''


class DeclarationTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def record(self, schema, name):
        return next(item for item in schema['records'] if item['name'] == name)

    def test_tags_and_caps_follow_the_closed_table(self):
        tags = {item['name']: item['id'] for item in self.schema['records']}
        self.assertEqual([tags[name] for name in DECLS], [5, 6, 7, 8, 9, 10])
        for name, fields in DECLS.items():
            record = self.record(self.schema, name)
            self.assertEqual([(f['name'], f['type'], f['max_bytes']) for f in record['fields']],
                             list(fields))
            for field in record['fields']:
                self.assertFalse(field['optional'])
                self.assertIsNone(field['default'])
                self.assertIsNone(field['ordinal'])
        names = [f['name'] for f in self.record(self.schema, 'resource.hyphenation')['fields']]
        self.assertEqual(names, ['language', 'path'])
        source = rust_records(self.schema)
        for name, fields in DECLS.items():
            for field, _kind, cap in fields:
                if cap:
                    const = f'{name.upper().replace(".", "_")}_{field.upper()}_MAX_BYTES'
                    self.assertIn(f'pub const {const}: usize = {cap};', source)

    def test_malformed_record_schemas_are_refused(self):
        def mutate(name, edit):
            candidate = copy.deepcopy(self.schema)
            edit(self.record(candidate, name))
            return candidate
        edits = {
            'optional': lambda r: r['fields'][0].update(optional=True),
            'ordinal': lambda r: r['fields'][0].update(ordinal=3),
            'default': lambda r: r['fields'][0].update(default='x'),
            'cap zero': lambda r: r['fields'][0].update(max_bytes=0),
            'cap too large': lambda r: r['fields'][0].update(max_bytes=4097),
            'bool cap': lambda r: r['fields'][0].update(max_bytes=True),
            'since too late': lambda r: r.update(since=2),
            'no fields': lambda r: r.update(fields=[]),
        }
        for name in DECLS:
            for label, edit in edits.items():
                with self.subTest(record=name, edit=label), self.assertRaises(ValueError):
                    validate_records(mutate(name, edit))

    def test_duplicate_or_oversized_tags_are_refused(self):
        candidate = copy.deepcopy(self.schema)
        self.record(candidate, 'setting.spec')['id'] = 5
        with self.assertRaises(ValueError):
            validate_records(candidate)
        candidate = copy.deepcopy(self.schema)
        self.record(candidate, 'setting.spec')['id'] = 65536
        with self.assertRaises(ValueError):
            validate_records(candidate)

    def test_utf8_status_is_required_with_declarations(self):
        candidate = copy.deepcopy(self.schema)
        candidate['statuses'] = [s for s in candidate['statuses'] if s['name'] != 'ERR_UTF8']
        with self.assertRaises(ValueError):
            validate_records(candidate)

    def test_declarations_do_not_change_the_registration_prefix(self):
        source = rust_records(self.schema)
        self.assertIn('pub fn registrations_decode', source)
        self.assertIn('pub fn records_decode', source)
        self.assertIn('pub fn records_encode', source)

    def test_standalone_vectors(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'decls.rs'
            source.write_text(rust_records(self.schema) + RUST_TESTS)
            binary = folder / 'decls-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
