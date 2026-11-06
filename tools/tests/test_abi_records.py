"""Registration/tick descriptors, generated coverage, and standalone wire vectors."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_records', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_records import rust_records, validate_records


RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;

    const EVENT: Registration = Registration::Event { event_tag: EVENT_TICK_TAG, token: 0x12345678 };
    const DRAW: Registration = Registration::Draw { token: u32::MAX };

    fn registrations(bytes: &[u8]) -> Result<std::vec::Vec<Registration>, i32> {
        registrations_decode(bytes).map(|batch| batch.iter().collect())
    }

    #[test]
    fn golden_vectors_and_roundtrips() {
        let expected = [2, 1, 9, 0, 3, 0, 0, 0, 0x78, 0x56, 0x34, 0x12,
                        2, 5, 0, 0xff, 0xff, 0xff, 0xff];
        let mut out = [0xa5; 32];
        assert_eq!(registrations_encode(&[EVENT, DRAW], &mut out), Ok(expected.len()));
        assert_eq!(&out[..expected.len()], expected);
        assert!(out[expected.len()..].iter().all(|&byte| byte == 0xa5));
        let batch = registrations_decode(&expected).unwrap();
        assert_eq!(batch.len(), 2);
        assert!(!batch.is_empty());
        assert_eq!(batch.iter().collect::<std::vec::Vec<_>>(), [EVENT, DRAW]);
        assert_eq!(registrations_decode(&[0]).unwrap().len(), 0);
        assert!(registrations_decode(&[0]).unwrap().is_empty());
        for count in 0..=REGISTRATION_MAX_RECORDS {
            let records = [EVENT; REGISTRATION_MAX_RECORDS];
            let mut out = [0; 128];
            let len = registrations_encode(&records[..count], &mut out).unwrap();
            assert_eq!(registrations(&out[..len]).unwrap(), records[..count]);
        }
        let tick = [1, 3, 9, 0, 8, 7, 6, 5, 4, 3, 2, 1];
        assert_eq!(tick_encode(0x0102030405060708, &mut out), Ok(tick.len()));
        assert_eq!(&out[..tick.len()], tick);
        assert_eq!(tick_decode(&tick), Ok(0x0102030405060708));
        for value in [0, 1, u64::MAX] {
            let len = tick_encode(value, &mut out).unwrap();
            assert_eq!(tick_decode(&out[..len]), Ok(value));
        }
    }

    #[test]
    fn output_is_unchanged_on_every_capacity_error() {
        let mut golden = [0; 128];
        for records in [&[][..], &[EVENT][..], &[EVENT, DRAW][..], &[DRAW; 8][..]] {
            let len = registrations_encode(records, &mut golden).unwrap();
            for cap in 0..len {
                let mut out = [0xa5; 128];
                assert_eq!(registrations_encode(records, &mut out[..cap]), Err(-3));
                assert_eq!(out, [0xa5; 128]);
            }
        }
        let mut out = [0xa5; 128];
        assert_eq!(registrations_encode(&[DRAW; 9], &mut out), Err(-3));
        assert_eq!(out, [0xa5; 128]);
        let len = tick_encode(1, &mut golden).unwrap();
        for cap in 0..len {
            assert_eq!(tick_encode(1, &mut out[..cap]), Err(-3));
            assert_eq!(out, [0xa5; 128]);
        }
    }

    #[test]
    fn truncated_and_malformed_frames() {
        let mut out = [0; 128];
        let len = registrations_encode(&[EVENT, DRAW], &mut out).unwrap();
        for cut in 0..len {
            assert_eq!(registrations(&out[..cut]), Err(-2));
        }
        let len = tick_encode(1, &mut out).unwrap();
        for cut in 0..len {
            assert_eq!(tick_decode(&out[..cut]), Err(-2));
        }
        let invalid = [
            &[][..], &[1], &[1, 2], &[1, 2, 0], // absent body bitmap
            &[1, 2, 1, 1], // bitmap longer than body
            &[1, 2, 5, 1, 0, 1, 2, 3], // required token incomplete
            &[1, 1, 5, 0, 1, 2, 3, 4], // missing event token
            &[1, 2, 5, 0, 1, 2, 3, 4, 0], // trailing outside record
            &[0, 0], // trailing outside empty batch
            &[1, 2, 0xff, 0xff, 0xff, 0xff, 0x0f], // enormous declared body
        ];
        for bytes in invalid {
            assert_eq!(registrations(bytes), Err(-2), "{bytes:?}");
        }
        assert_eq!(tick_decode(&[0]), Err(-2));
        assert_eq!(tick_decode(&[2]), Err(-2));
        let mut tick = std::vec![1, 3, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        tick.push(0);
        assert_eq!(tick_decode(&tick), Err(-2));
    }

    #[test]
    fn canonical_unsigned_u32_leb128_every_header_position() {
        let invalid = [
            &[0x80, 0][..], &[0x81, 0], &[0xff, 0xff, 0xff, 0xff, 0x10],
            &[0x80, 0x80, 0x80, 0x80, 0x80, 0], &[0x80],
        ];
        for varint in invalid {
            for prefix in [&[][..], &[1][..], &[1, 2][..], &[1, 2, 12][..]] {
                let mut bytes = prefix.to_vec();
                bytes.extend_from_slice(varint);
                if prefix.len() == 3 {
                    bytes.resize(15, 0); // Complete record: malformed bitmap, not truncation.
                }
                assert_eq!(registrations(&bytes), Err(-2), "{bytes:?}");
            }
            for prefix in [&[][..], &[1][..], &[1, 3][..], &[1, 3, 12][..]] {
                let mut bytes = prefix.to_vec();
                bytes.extend_from_slice(varint);
                if prefix.len() == 3 {
                    bytes.resize(15, 0);
                }
                assert_eq!(tick_decode(&bytes), Err(-2), "{bytes:?}");
            }
        }
    }

    #[test]
    fn limits_unknown_tags_and_no_partial_batch() {
        assert_eq!(registrations(&[9]), Err(-3));
        assert_eq!(registrations(&[0xff, 0xff, 0xff, 0xff, 0x0f]), Err(-3));
        assert_eq!(registrations(&[1, 3, 0]), Err(-1));
        assert_eq!(tick_decode(&[1, 2, 0]), Err(-1));
        assert_eq!(registrations(&[2, 2, 5, 0, 1, 2, 3, 4, 99, 0]), Err(-1));
        let too_large = [0; REGISTRATION_MAX_BYTES + 1];
        assert_eq!(registrations(&too_large), Err(-3));
        assert_eq!(tick_decode(&too_large), Err(-3));
        // A maximal canonical u32 tag is supported framing, but an unknown record.
        assert_eq!(registrations(&[1, 0xff, 0xff, 0xff, 0xff, 0x0f, 0]), Err(-1));
    }

    fn extended(tag: u8, scalar: &[u8], total: usize) -> std::vec::Vec<u8> {
        // A two-byte canonical length covers the entire forward-compatible body.
        let body_len = total - 4;
        let mut bytes = std::vec![1, tag, (body_len as u8 & 0x7f) | 0x80,
                                 (body_len >> 7) as u8, 2, 0x80, 0xff];
        bytes.extend_from_slice(scalar);
        bytes.resize(total, 0xf1);
        bytes
    }

    #[test]
    fn forward_fields_and_extended_bitmaps_preserve_required_values() {
        let draw = [1, 2, 8, 2, 0x80, 0xff, 4, 3, 2, 1, 99];
        assert_eq!(registrations(&draw), Ok(std::vec![Registration::Draw { token: 0x01020304 }]));
        let event = [1, 1, 12, 1, 1, 3, 0, 0, 0, 7, 0, 0, 0, 42, 99];
        assert_eq!(registrations(&event), Ok(std::vec![Registration::Event {
            event_tag: 3, token: 7 }]));
        let tick = [1, 3, 11, 1, 1, 8, 7, 6, 5, 4, 3, 2, 1, 42];
        assert_eq!(tick_decode(&tick), Ok(0x0102030405060708));
        let max_draw = extended(2, &7_u32.to_le_bytes(), REGISTRATION_MAX_BYTES);
        assert_eq!(registrations(&max_draw), Ok(std::vec![Registration::Draw { token: 7 }]));
        let max_tick = extended(3, &7_u64.to_le_bytes(), REGISTRATION_MAX_BYTES);
        assert_eq!(tick_decode(&max_tick), Ok(7));
    }

    #[test]
    fn arbitrary_short_inputs_never_panic() {
        for first in 0..=255 {
            for second in 0..=255 {
                let bytes = [first, second];
                let _registration_result = registrations_decode(&bytes);
                let _tick_result = tick_decode(&bytes);
            }
        }
        let mut state = 0x12345678_u32;
        for len in 0..128 {
            let mut bytes = [0; 128];
            for byte in &mut bytes[..len] {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                *byte = state as u8;
            }
            let _registration_result = registrations_decode(&bytes[..len]);
            let _tick_result = tick_decode(&bytes[..len]);
        }
    }
}
'''


class RecordsTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def test_initial_descriptors_and_tags(self):
        ABI.validate(self.schema)
        self.assertEqual([(r['name'], r['id']) for r in self.schema['records']],
                         [('registration.event', 1), ('registration.draw', 2), ('event.tick', 3),
                          ('command.set_setting', 4), ('setting.group', 5),
                          ('setting.spec', 6), ('setting.extra', 7),
                          ('resource.hyphenation', 8), ('resource.font_face', 9),
                          ('resource.font_extension', 10)])
        changed = copy.deepcopy(self.schema)
        for record in changed['records']:
            record['id'] += 127
        source = rust_records(changed)
        self.assertIn('EVENT_TICK_TAG: u32 = 130;', source)
        self.assertIn('REGISTRATION_EVENT_TAG: u32 = 128;', source)
        self.assertIn('REGISTRATION_DRAW_TAG: u32 = 129;', source)
        before = copy.deepcopy(self.schema)
        before['records'] = []
        del before['record_codec']
        del before['command_codec']
        self.assertEqual(rust_records(before), '')
        ABI.compatible(before, self.schema, 0)
        with self.assertRaisesRegex(ValueError, 'backdated'):
            ABI.compatible(before, self.schema, 1)
        later = copy.deepcopy(self.schema)
        later['minor'] = 2
        later['record_codec']['since'] = 2
        later['command_codec']['since'] = 2
        for record in later['records']:
            record['since'] = 2
            for field in record['fields']:
                field['since'] = 2
        ABI.compatible(before, later, 1)

    def test_known_shapes_are_validated(self):
        mutations = [('since', 2), ('fields', []), ('fields', None)]
        for key, value in mutations:
            candidate = copy.deepcopy(self.schema)
            candidate['minor'] = 2
            candidate['records'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for key, value in [('type', 'u64'), ('name', 'other'), ('since', True),
                           ('optional', True), ('ordinal', 0), ('default', 0),
                           ('max_bytes', True), ('max_bytes', 4)]:
            candidate = copy.deepcopy(self.schema)
            candidate['records'][0]['fields'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for index in range(3):
            candidate = copy.deepcopy(self.schema)
            candidate['records'].pop(index)
            with self.assertRaises(ValueError):
                ABI.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['records'][0]['fields'].reverse()
        with self.assertRaises(ValueError):
            ABI.validate(candidate)

    def test_count_descriptor_is_closed_and_frozen(self):
        for value in [None, {}, {'since': 1, 'max_records': 8, 'extra': 0},
                      {'since': True, 'max_records': 8}, {'since': 2, 'max_records': 8},
                      {'since': 1, 'max_records': True}, {'since': 1, 'max_records': 0},
                      {'since': 1, 'max_records': 9}]:
            candidate = copy.deepcopy(self.schema)
            candidate['record_codec'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                ABI.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['record_codec']['max_records'] = 4
        ABI.validate(candidate)
        self.assertIn('REGISTRATION_MAX_RECORDS: usize = 4;', rust_records(candidate))
        with self.assertRaisesRegex(ValueError, 'record codec contract'):
            ABI.compatible(self.schema, candidate)

    def test_lock_pins_standalone_emission(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['emitted_records'], rust_records(self.schema))
        original = ABI.rust_records
        with patch.object(ABI, 'rust_records',
                          lambda schema: original(schema).replace('return Err(-2)', 'return Err(-3)')):
            ABI.check_lock(ABI.lock_record(self.schema), self.schema)
            with self.assertRaisesRegex(ValueError, 'emitted records contract'):
                ABI.check_lock(trusted, self.schema)
        # The original raw module and settings emission do not acquire these types.
        self.assertNotIn('RegistrationBatch', ABI.rust_module(self.schema))

    def test_standalone_copies_generated_and_checked(self):
        source = rust_records(self.schema)
        for path in ['abi/generated/records_wire.rs',
                     'crates/paperpaw-core/src/abi7/records_wire.rs']:
            self.assertEqual((ROOT / path).read_text(), source)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            (root / 'abi/abi.def').write_text(ABI.canonical(self.schema))
            ABI.run(['--root', str(root), '--write-lock'])
            for path in ['abi/generated/records_wire.rs',
                         'crates/paperpaw-core/src/abi7/records_wire.rs']:
                generated = root / path
                self.assertEqual(generated.read_text(), source)
                generated.write_text('// stale')
                with self.assertRaisesRegex(ValueError, 'stale generated file'):
                    ABI.run(['--root', str(root), '--check'])
                ABI.run(['--root', str(root)])
            ABI.run(['--root', str(root), '--check'])

    def test_standalone_rust_no_std_and_vectors(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'records.rs'
            source.write_text('#![no_std]\n' + rust_records(self.schema))
            subprocess.run([rustc, '--edition=2024', '--crate-type=lib', '-Dwarnings',
                            str(source), '-o', str(folder / 'records.rlib')], check=True)
            source.write_text(rust_records(self.schema) + RUST_TESTS)
            binary = folder / 'records-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
