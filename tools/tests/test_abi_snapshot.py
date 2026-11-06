"""Snapshot descriptor policy, generated coverage and standalone Rust vectors."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_snapshot', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_snapshot import rust_snapshot
from abi_requirements import required_minor
from abi_metadata import encode, decode

RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;
    use core::mem::MaybeUninit;

    fn base() -> Snapshot {
        Snapshot::new(480, 800, 220, PixelFormat::Mono1)
    }

    fn encode(snapshot: &Snapshot) -> [u8; SNAPSHOT_BYTES] {
        let mut out = [MaybeUninit::uninit(); SNAPSHOT_BYTES];
        assert_eq!(snapshot_encode(snapshot, &mut out), Ok(SNAPSHOT_BYTES));
        // Every prefix byte is initialized after successful encoding.
        out.map(|byte| unsafe { byte.assume_init() })
    }

    #[test]
    fn canonical_golden_and_absent_is_distinct_from_zero() {
        let golden = [
            1,0,0,0, 52,0,0,0, 0,0,0,0, 0,0,0,0,
            0,0,0,0,0,0,0,0, 0,0,0,0,0,0,0,0,
            0xe0,1,0x20,3,0xdc,0, 0,0,0,0,1,0, 0,0,0,0,0,0,0,0,
        ];
        let mut snapshot = base();
        assert_eq!(encode(&snapshot), golden);
        assert_eq!(snapshot_decode(&golden), Ok(snapshot));
        snapshot.battery = Some(Battery::new(0));
        snapshot.unix_seconds = Some(0);
        let mut zero = golden;
        zero[8] = 5;
        assert_eq!(encode(&snapshot), zero);
        assert_eq!(snapshot_decode(&zero), Ok(snapshot));
        snapshot.battery.as_mut().unwrap().millivolts = Some(0);
        zero[8] = 7;
        assert_eq!(encode(&snapshot), zero);
        assert_eq!(snapshot_decode(&zero), Ok(snapshot));
        snapshot.monotonic_ms = u64::MAX;
        snapshot.unix_seconds = Some(-1);
        snapshot.battery = Some(Battery {
            percent: 100, charging: true, saver: true, millivolts: Some(u16::MAX),
        });
        snapshot.touch = true;
        let bytes = encode(&snapshot);
        assert_eq!(&bytes[16..32], &[255; 16]);
        assert_eq!(&bytes[43..49], &[100, 255, 255, 1, 1, 1]);
        assert_eq!(snapshot_decode(&bytes), Ok(snapshot));
        for unix in [i64::MIN, -23, 0, i64::MAX] {
            snapshot.unix_seconds = Some(unix);
            assert_eq!(snapshot_decode(&encode(&snapshot)), Ok(snapshot));
        }
    }

    #[test]
    fn every_pixel_format_and_exact_sdk_depth_semantics() {
        for (index, (pixel, bits, levels)) in [
            (PixelFormat::Mono1, 1, Some(2)),
            (PixelFormat::Gray2, 2, Some(4)),
            (PixelFormat::Gray4, 4, Some(16)),
            (PixelFormat::Gray8, 8, Some(256)),
            (PixelFormat::Rgb565, 16, None),
            (PixelFormat::Rgb888, 24, None),
            (PixelFormat::Other { code: 0, bits: 0 }, 0, None),
            (PixelFormat::Other { code: u16::MAX, bits: u8::MAX }, 255, None),
        ].into_iter().enumerate() {
            assert_eq!(pixel.bits_per_pixel(), bits);
            assert_eq!(pixel.gray_levels(), levels);
            let mut snapshot = base();
            snapshot.pixel = pixel;
            let bytes = encode(&snapshot);
            assert_eq!(bytes[38], index.min(6) as u8);
            assert_eq!(bytes[42], bits);
            assert_eq!(snapshot_decode(&bytes), Ok(snapshot));
        }
        for tag in 0..6 {
            let mut bytes = encode(&base());
            bytes[38] = tag;
            bytes[42] = [1, 2, 4, 8, 16, 24][tag as usize];
            bytes[40] = 1;
            assert_eq!(snapshot_decode(&bytes), Err(-2));
            bytes[40] = 0;
            bytes[42] = 0;
            assert_eq!(snapshot_decode(&bytes), Err(-2));
        }
        let mut bytes = encode(&base());
        bytes[38..40].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(snapshot_decode(&bytes), Err(-1));
    }

    #[test]
    fn every_short_capacity_is_untouched_and_success_writes_only_prefix() {
        for cap in 0..SNAPSHOT_BYTES {
            let mut out = [MaybeUninit::new(0xa5); SNAPSHOT_MAX_BYTES];
            assert_eq!(snapshot_encode(&base(), &mut out[..cap]), Err(-3));
            assert!(out.into_iter().all(|byte| unsafe { byte.assume_init() } == 0xa5));
        }
        let mut out = [MaybeUninit::new(0xa5); SNAPSHOT_MAX_BYTES];
        assert_eq!(snapshot_encode(&base(), &mut out), Ok(SNAPSHOT_BYTES));
        assert!(out[SNAPSHOT_BYTES..].iter().all(|byte| unsafe { byte.assume_init() } == 0xa5));
        let mut invalid = base();
        invalid.battery = Some(Battery::new(101));
        for cap in 0..=SNAPSHOT_MAX_BYTES {
            let mut out = [MaybeUninit::new(0xa5); SNAPSHOT_MAX_BYTES];
            assert_eq!(snapshot_encode(&invalid, &mut out[..cap]), Err(-2));
            assert!(out.into_iter().all(|byte| unsafe { byte.assume_init() } == 0xa5));
        }
    }

    #[test]
    fn strict_presence_boolean_percent_and_reserved_validation() {
        let canonical = encode(&base());
        for offset in [12, 13, 14, 15, 24, 31, 43, 44, 45, 46, 47, 49, 50, 51] {
            let mut bytes = canonical;
            bytes[offset] = 1;
            assert_eq!(snapshot_decode(&bytes), Err(-2), "offset {offset}");
        }
        for presence in [2, 6] {
            let mut bytes = canonical;
            bytes[8] = presence;
            assert_eq!(snapshot_decode(&bytes), Err(-2));
        }
        for bit in 3..32 {
            let mut bytes = canonical;
            bytes[8..12].copy_from_slice(&(1_u32 << bit).to_le_bytes());
            assert_eq!(snapshot_decode(&bytes), Err(-1));
        }
        for offset in [46, 47, 48] {
            for value in 2..=255 {
                let mut bytes = canonical;
                bytes[8] = 1;
                bytes[offset] = value;
                assert_eq!(snapshot_decode(&bytes), Err(-2));
            }
        }
        for percent in 101..=255 {
            let mut bytes = canonical;
            bytes[8] = 1;
            bytes[43] = percent;
            assert_eq!(snapshot_decode(&bytes), Err(-2));
        }
    }

    #[test]
    fn length_version_extensions_and_caps() {
        let bytes = encode(&base());
        for len in 0..SNAPSHOT_BYTES {
            assert_eq!(snapshot_decode(&bytes[..len]), Err(-2));
        }
        for total in [0, 51, 53, 128, u32::MAX] {
            let mut invalid = bytes;
            invalid[4..8].copy_from_slice(&total.to_le_bytes());
            assert_eq!(snapshot_decode(&invalid), Err(-2));
        }
        for version in [0_u32, 2, u32::MAX] {
            let mut invalid = bytes;
            invalid[..4].copy_from_slice(&version.to_le_bytes());
            assert_eq!(snapshot_decode(&invalid), Err(-1));
        }
        for len in SNAPSHOT_BYTES..=SNAPSHOT_MAX_BYTES {
            let mut extended = [0xa5; SNAPSHOT_MAX_BYTES];
            extended[..SNAPSHOT_BYTES].copy_from_slice(&bytes);
            extended[4..8].copy_from_slice(&(len as u32).to_le_bytes());
            assert_eq!(snapshot_decode(&extended[..len]), Ok(base()));
        }
        assert_eq!(snapshot_decode(&[0; SNAPSHOT_MAX_BYTES + 1]), Err(-3));
        // All byte values at each prefix position must remain safe to inspect.
        for offset in 0..SNAPSHOT_BYTES {
            for value in 0..=255 {
                let mut changed = bytes;
                changed[offset] = value;
                let _result = snapshot_decode(&changed);
            }
        }
    }
}
'''


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def test_host_requirement_includes_initial_codec_floor(self):
        prior = copy.deepcopy(self.schema)
        del prior['snapshot_value']
        trusted = ABI.lock_record(prior, released_minor=1)
        candidate = copy.deepcopy(self.schema)
        candidate['minor'] = 2
        candidate['snapshot_value']['since'] = 2
        ABI.check_lock(trusted, candidate)
        used = [dict(kind='host', name='env.snapshot')]
        self.assertEqual(required_minor(candidate, used, 0), 2)
        self.assertEqual(required_minor(candidate, [dict(kind='host', name='heap.alloc')], 0), 1)
        build = dict(target=candidate['metadata']['profiles'][0]['name'], used=used,
                     floor=0, services=[], features=[])
        self.assertEqual(decode(candidate, encode(candidate, build))['needs_minor'], 2)

    def test_descriptor_and_derived_tags(self):
        ABI.validate(self.schema)
        older = copy.deepcopy(self.schema)
        del older['snapshot_value']
        self.assertEqual(rust_snapshot(older), '')
        self.assertEqual(ABI.rust_module(older), ABI.rust_module(self.schema))
        changed = copy.deepcopy(self.schema)
        for pixel in changed['snapshot_value']['pixels']:
            pixel['tag'] += 100
        self.assertIn('Self::Mono1 => 100,', rust_snapshot(changed))
        self.assertIn('106 => return Ok(PixelFormat::Other', rust_snapshot(changed))

    def test_closed_descriptor_and_invalid_types(self):
        for key, value in [('since', True), ('since', 0), ('since', 2), ('version', True),
                           ('version', 2), ('max_bytes', True), ('max_bytes', 129),
                           ('byte_order', 'big'), ('fields', []), ('header', []),
                           ('presence', []), ('pixels', [])]:
            candidate = copy.deepcopy(self.schema)
            candidate['snapshot_value'][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                ABI.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['snapshot_value']['extra'] = 0
        with self.assertRaises(ValueError):
            ABI.validate(candidate)
        for key in ['header', 'fields', 'presence', 'pixels']:
            candidate = copy.deepcopy(self.schema)
            candidate['snapshot_value'][key][0]['extra'] = 0
            with self.subTest(key=key), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for key, field in [('presence', 'bit'), ('pixels', 'tag'), ('pixels', 'bits')]:
            candidate = copy.deepcopy(self.schema)
            candidate['snapshot_value'][key][0][field] = True
            with self.assertRaises(ValueError):
                ABI.validate(candidate)
        for section, key, value in [('host_entries', 'max_bytes', 129),
                                    ('statuses', 'value', -99)]:
            candidate = copy.deepcopy(self.schema)
            index = 4 if section == 'host_entries' else 1
            candidate[section][index][key] = value
            with self.assertRaises(ValueError):
                ABI.validate(candidate)

    def test_release_floor_initial_addition_and_immutable_existing(self):
        before = copy.deepcopy(self.schema)
        del before['snapshot_value']
        ABI.compatible(before, self.schema, 0)
        with self.assertRaisesRegex(ValueError, 'backdated snapshot'):
            ABI.compatible(before, self.schema, 1)
        later = copy.deepcopy(self.schema)
        later['minor'] = 2
        later['snapshot_value']['since'] = 2
        ABI.compatible(before, later, 1)
        with self.assertRaisesRegex(ValueError, 'snapshot value contract'):
            ABI.compatible(self.schema, later, 0)
        with self.assertRaisesRegex(ValueError, 'snapshot value contract'):
            ABI.compatible(self.schema, before, 0)

    def test_lock_coverage_and_legacy_formats(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['lock_format'], 8)
        self.assertEqual(trusted['emitted_snapshot'], rust_snapshot(self.schema))
        for version in range(1, 9):
            legacy = copy.deepcopy(trusted)
            legacy['lock_format'] = version
            for floor, field in [(2, 'emitted_rust'), (3, 'emitted_metadata'),
                                 (4, 'emitted_records'), (5, 'emitted_snapshot'),
                                 (6, 'emitted_color'), (7, 'emitted_commands'),
                                 (8, 'emitted_streams')]:
                if version < floor:
                    del legacy[field]
            ABI.check_lock(legacy, self.schema)
        original = ABI.rust_snapshot
        with patch.object(ABI, 'rust_snapshot', lambda s: original(s) + '// drift\n'):
            with self.assertRaisesRegex(ValueError, 'emitted snapshot contract'):
                ABI.check_lock(trusted, self.schema)

    def test_owned_generated_files_and_stale_detection(self):
        source = rust_snapshot(self.schema)
        paths = ['abi/generated/snapshot_wire.rs',
                 'crates/paperpaw-core/src/abi7/snapshot_wire.rs']
        for path in paths:
            self.assertEqual((ROOT / path).read_text(), source)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'abi').mkdir()
            (root / 'abi/abi.def').write_text(ABI.canonical(self.schema))
            ABI.run(['--root', str(root), '--write-lock'])
            for path in paths:
                generated = root / path
                self.assertEqual(generated.read_text(), source)
                generated.write_text('// stale')
                with self.assertRaisesRegex(ValueError, 'stale generated file'):
                    ABI.run(['--root', str(root), '--check'])
                ABI.run(['--root', str(root)])
            ABI.run(['--root', str(root), '--check'])

    def test_standalone_no_std_and_wire_vectors(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'snapshot.rs'
            source.write_text('#![no_std]\n' + rust_snapshot(self.schema))
            subprocess.run([rustc, '--edition=2024', '--crate-type=lib', '-Dwarnings',
                            str(source), '-o', str(folder / 'snapshot.rlib')], check=True)
            source.write_text(rust_snapshot(self.schema) + RUST_TESTS)
            binary = folder / 'snapshot-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
