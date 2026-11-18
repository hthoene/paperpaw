"""Compile and exercise the generated no_std codec without workspace builds."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    fn wire(kind: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    fn encoded(value: SettingValueRef<'_>) -> Vec<u8> {
        let mut bytes = vec![MaybeUninit::new(0xaa); SETTINGS_MAX_BYTES + 2];
        let required = settings_encode(value, &mut bytes).unwrap();
        bytes[..required].iter().map(|byte| unsafe { byte.assume_init() }).collect()
    }

    fn roundtrip(value: SettingValueRef<'_>, expected: Vec<u8>) {
        assert_eq!(encoded(value), expected);
        assert_eq!(settings_decode(&expected), Ok(value));
        // Initialized sentinel bytes let this also verify that the suffix is untouched.
        for cap in 0..=expected.len() + 2 {
            let mut out = vec![MaybeUninit::new(0xaa); cap + 2];
            assert_eq!(settings_encode(value, &mut out[..cap]), Ok(expected.len()));
            let bytes: Vec<u8> = out.iter().map(|byte| unsafe { byte.assume_init() }).collect();
            let written = cap.min(expected.len());
            assert_eq!(&bytes[..written], &expected[..written]);
            assert!(bytes[written..].iter().all(|byte| *byte == 0xaa));
        }
    }

    #[test]
    fn all_types_and_signed_extremes_have_canonical_little_endian_bytes() {
        roundtrip(SettingValueRef::Absent, wire(0, &[]));
        roundtrip(SettingValueRef::Bool(false), wire(1, &[0]));
        roundtrip(SettingValueRef::Bool(true), wire(1, &[1]));
        for value in [i64::MIN, -257, -1, 0, 1, 257, i64::MAX] {
            roundtrip(SettingValueRef::Int(value), wire(2, &value.to_le_bytes()));
        }
        for value in ["", "a", "é🦀", "contains\0nul"] {
            roundtrip(SettingValueRef::Text(value), wire(3, value.as_bytes()));
        }
    }

    #[test]
    fn maximum_text_and_partial_output_are_bounded() {
        assert_eq!(SETTINGS_LAYOUT, 1);
        assert_eq!(SETTINGS_HEADER_BYTES, 12);
        assert_eq!(SETTINGS_MAX_BYTES, 4096);
        assert_eq!(SETTINGS_MAX_TEXT_BYTES, 4084);
        for text in ["a".repeat(4084), "é".repeat(2042)] {
            let expected = wire(3, text.as_bytes());
            roundtrip(SettingValueRef::Text(&text), expected.clone());
            let SettingValueRef::Text(borrowed) = settings_decode(&expected).unwrap() else {
                panic!("text expected");
            };
            assert_eq!(borrowed.as_ptr(), expected[12..].as_ptr());
        }
    }

    #[test]
    fn oversized_encode_writes_nothing_even_with_a_short_output() {
        for text in ["x".repeat(4085), "é".repeat(2043)] {
            for cap in [0, 1, 12, 4096, 5000] {
                let mut out = vec![MaybeUninit::new(0xab); cap];
                assert_eq!(settings_encode(SettingValueRef::Text(&text), &mut out), Err(-3));
                assert!(out.iter().all(|byte| unsafe { byte.assume_init() } == 0xab));
            }
        }
    }

    #[test]
    fn framing_requires_a_complete_exact_payload() {
        for bytes in [wire(0, &[]), wire(1, &[1]), wire(2, &0_i64.to_le_bytes()),
                      wire(3, b"text")] {
            for length in 0..bytes.len() {
                assert_eq!(settings_decode(&bytes[..length]), Err(-2));
            }
            let mut trailing = bytes.clone();
            trailing.push(0);
            assert_eq!(settings_decode(&trailing), Err(-2));
            for declared in [0, 1, 8, 4084, u32::MAX] {
                if declared as usize == bytes.len() - 12 { continue; }
                let mut wrong = bytes.clone();
                wrong[8..12].copy_from_slice(&declared.to_le_bytes());
                assert_eq!(settings_decode(&wrong), Err(-2));
            }
        }
        assert_eq!(settings_decode(&wire(3, &vec![b'x'; 4085])), Err(-2));
    }

    #[test]
    fn malformed_scalars_invalid_text_and_unknown_discriminants_are_distinct() {
        for payload in [&[0][..], &[1, 2][..]] {
            assert_eq!(settings_decode(&wire(0, payload)), Err(-2));
        }
        for payload in [&[][..], &[2][..], &[255][..], &[0, 0][..]] {
            assert_eq!(settings_decode(&wire(1, payload)), Err(-2));
        }
        for count in [0, 1, 7, 9] {
            assert_eq!(settings_decode(&wire(2, &vec![0; count])), Err(-2));
        }
        for payload in [&[0x80][..], &[0xc0, 0x80][..], &[0xf0, 0x9f][..],
                        &[0xed, 0xa0, 0x80][..]] {
            assert_eq!(settings_decode(&wire(3, payload)), Err(-5));
        }
        for kind in [4, 255, u32::MAX] {
            assert_eq!(settings_decode(&wire(kind, &[])), Err(-1));
        }
        for layout in [0_u32, 2, u32::MAX] {
            let mut bytes = wire(0, &[]);
            bytes[..4].copy_from_slice(&layout.to_le_bytes());
            assert_eq!(settings_decode(&bytes), Err(-1));
        }
    }

    #[test]
    fn arbitrary_input_cannot_panic() {
        let mut seed = 0x12345678_u32;
        for length in 0..=4100 {
            let mut bytes = Vec::new();
            for _ in 0..length {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                bytes.push(seed as u8);
            }
            let _result = settings_decode(&bytes);
            // Also reach the canonical-header branch with arbitrary payloads.
            if length >= 12 {
                bytes[..4].copy_from_slice(&1_u32.to_le_bytes());
                bytes[4..8].copy_from_slice(&(length as u32 % 4).to_le_bytes());
                bytes[8..12].copy_from_slice(&((length - 12) as u32).to_le_bytes());
                let _result = settings_decode(&bytes);
            }
        }
    }
}
'''


class SettingsWireTests(unittest.TestCase):
    def test_generated_codec_compiles_without_std_and_passes_wire_tests(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        env = dict(os.environ)
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            codec = (ROOT / 'abi/generated/settings_value.rs').read_text()
            source = folder / 'settings.rs'
            source.write_text('#![no_std]\n' + codec)
            subprocess.run([rustc, '--edition=2024', '--crate-type=lib', '-Dwarnings',
                            str(source), '-o', str(folder / 'settings.rlib')],
                           check=True, cwd=ROOT, env=env, capture_output=True, text=True)
            source.write_text(codec + RUST_TESTS)
            binary = folder / 'settings-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True, cwd=ROOT, env=env,
                           capture_output=True, text=True)
            result = subprocess.run([str(binary)], check=True, cwd=ROOT,
                                    capture_output=True, text=True)
            self.assertIn('6 passed', result.stdout)


if __name__ == '__main__':
    unittest.main()
