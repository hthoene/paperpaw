"""Packed-color descriptor policy, requirement floors and Rust golden values."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_color', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_color import rust_color
from abi_requirements import required_minor
from abi_metadata import encode, decode

RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_srgb_golden_values_and_channel_order() {
        const RED: u32 = color_encode(255, 0, 0);
        const MIXED: Result<[u8; 3], i32> = color_decode(0x00123456);
        assert_eq!(RED, 0x00ff0000);
        assert_eq!(MIXED, Ok([0x12, 0x34, 0x56]));
        for (channels, packed) in [
            ([0, 0, 0], 0x00000000),
            ([255, 255, 255], 0x00ffffff),
            ([255, 0, 0], 0x00ff0000),
            ([0, 255, 0], 0x0000ff00),
            ([0, 0, 255], 0x000000ff),
            ([0x12, 0x34, 0x56], 0x00123456),
            ([0x7f, 0x80, 0xfe], 0x007f80fe),
        ] {
            assert_eq!(color_encode(channels[0], channels[1], channels[2]), packed);
            assert_eq!(color_decode(packed), Ok(channels));
        }
    }

    #[test]
    fn every_channel_byte_round_trips_without_quantization() {
        for byte in 0..=255 {
            for channels in [[byte, 0x34, 0x56], [0x12, byte, 0x56], [0x12, 0x34, byte]] {
                let packed = color_encode(channels[0], channels[1], channels[2]);
                assert_eq!(packed & 0xff000000, 0);
                assert_eq!(color_decode(packed), Ok(channels));
            }
        }
    }

    #[test]
    fn every_nonzero_high_byte_is_invalid_and_never_alpha() {
        for reserved in 1_u32..=255 {
            for rgb in [0, 0x00123456, 0x00ffffff] {
                assert_eq!(color_decode((reserved << 24) | rgb), Err(-2));
            }
        }
        assert_eq!(color_decode(u32::MAX), Err(-2));
    }
}
'''


class ColorTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def test_optional_descriptor_and_unchanged_raw_declarations(self):
        older = copy.deepcopy(self.schema)
        del older['color_value']
        ABI.validate(older)
        self.assertEqual(rust_color(older), '')
        self.assertEqual(ABI.rust_module(older), ABI.rust_module(self.schema))
        self.assertEqual(ABI.header(older), ABI.header(self.schema))

    def test_closed_descriptor_and_strict_scalar_values(self):
        mutations = [('since', True), ('since', 0), ('since', 2), ('since', '1'),
                     ('type', 'u64'), ('color_space', 'linear'), ('alpha', True),
                     ('alpha', 'straight'), ('reserved_mask', True),
                     ('reserved_mask', 0), ('reserved_mask', 0xff000000 + 1),
                     ('invalid_status', 'ERR_UNSUPPORTED'), ('channels', []),
                     ('extra', 0)]
        for key, value in mutations:
            candidate = copy.deepcopy(self.schema)
            candidate['color_value'][key] = value
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for key in self.schema['color_value']:
            candidate = copy.deepcopy(self.schema)
            del candidate['color_value'][key]
            with self.subTest(missing=key), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for key, value in [('name', 'blue'), ('shift', True), ('shift', 8),
                           ('bits', True), ('bits', 16), ('extra', 0)]:
            candidate = copy.deepcopy(self.schema)
            candidate['color_value']['channels'][0][key] = value
            with self.subTest(channel_key=key), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for value in [None, [], True, 'rgb']:
            candidate = copy.deepcopy(self.schema)
            candidate['color_value'] = value
            with self.subTest(descriptor=value), self.assertRaises(ValueError):
                ABI.validate(candidate)

    def test_color_argument_and_status_contract(self):
        color_hosts = [entry for entry in self.schema['host_entries']
                       if any(arg['name'] == 'color' for arg in entry['args'])]
        self.assertEqual([entry['id'] for entry in color_hosts], [9, 10, 11, 12, 14, 41])
        for entry in color_hosts:
            candidate = copy.deepcopy(self.schema)
            host = next(item for item in candidate['host_entries'] if item['id'] == entry['id'])
            next(arg for arg in host['args'] if arg['name'] == 'color')['type'] = 'i32'
            with self.subTest(host=entry['name']), self.assertRaises(ValueError):
                ABI.validate(candidate)
        for value in [-99, True]:
            candidate = copy.deepcopy(self.schema)
            next(item for item in candidate['statuses']
                 if item['name'] == 'ERR_INVALID')['value'] = value
            with self.assertRaises(ValueError):
                ABI.validate(candidate)
        candidate = copy.deepcopy(self.schema)
        candidate['minor'] = 2
        next(item for item in candidate['statuses']
             if item['name'] == 'ERR_INVALID')['since'] = 2
        with self.assertRaises(ValueError):
            ABI.validate(candidate)

    def test_release_floor_and_existing_contract_immutability(self):
        older = copy.deepcopy(self.schema)
        del older['color_value']
        ABI.compatible(older, self.schema, 0)
        with self.assertRaisesRegex(ValueError, 'backdated color'):
            ABI.compatible(older, self.schema, 1)
        later = copy.deepcopy(self.schema)
        later['minor'] = 2
        later['color_value']['since'] = 2
        ABI.compatible(older, later, 1)
        for candidate in [older, later]:
            with self.assertRaisesRegex(ValueError, 'color value contract'):
                ABI.compatible(self.schema, candidate, 0)

    def test_all_color_hosts_automatically_aggregate_minor_and_metadata_floor(self):
        candidate = copy.deepcopy(self.schema)
        candidate['minor'] = 2
        candidate['color_value']['since'] = 2
        for entry in candidate['host_entries']:
            used = [dict(kind='host', name=entry['name'])]
            expected = 2 if any(arg['name'] == 'color' for arg in entry['args']) else 1
            self.assertEqual(required_minor(candidate, used, 0), expected, entry['name'])
            build = dict(target=candidate['metadata']['profiles'][0]['name'], used=used,
                         floor=0, services=[], features=[])
            self.assertEqual(decode(candidate, encode(candidate, build))['needs_minor'], expected)
        # This is semantic, not a list of the current graphics slot numbers.
        extra = copy.deepcopy(candidate['host_entries'][9])
        extra.update(id=20, name='canvas.future')
        candidate['host_entries'].append(extra)
        self.assertEqual(required_minor(candidate, [dict(kind='host', name=extra['name'])], 0), 2)
        del candidate['color_value']
        self.assertEqual(required_minor(candidate, [dict(kind='host', name='text.draw')], 0), 1)

    def test_lock_coverage_and_legacy_formats_without_descriptor(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['lock_format'], 11)
        self.assertEqual(trusted['emitted_color'], rust_color(self.schema))
        older = copy.deepcopy(self.schema)
        del older['color_value']
        for version in range(1, 12):
            legacy = ABI.lock_record(older)
            legacy['lock_format'] = version
            for floor, field in [(2, 'emitted_rust'), (3, 'emitted_metadata'),
                                 (4, 'emitted_records'), (5, 'emitted_snapshot'),
                                 (6, 'emitted_color'), (7, 'emitted_commands'),
                                 (8, 'emitted_streams'), (9, 'emitted_services'),
                                 (10, 'emitted_host_decode'), (11, 'emitted_stages')]:
                if version < floor:
                    del legacy[field]
            ABI.check_lock(legacy, self.schema)
        original = ABI.rust_color
        with patch.object(ABI, 'rust_color', lambda schema: original(schema) + '// drift\n'):
            with self.assertRaisesRegex(ValueError, 'emitted color contract'):
                ABI.check_lock(trusted, self.schema)

    def test_owned_generated_files_and_stale_detection(self):
        source = rust_color(self.schema)
        paths = ['abi/generated/color_wire.rs', 'crates/paperpaw-core/src/abi7/color_wire.rs']
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

    def test_standalone_no_std_and_golden_values(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'color.rs'
            source.write_text('#![no_std]\n' + rust_color(self.schema))
            subprocess.run([rustc, '--edition=2024', '--crate-type=lib', '-Dwarnings',
                            str(source), '-o', str(folder / 'color.rlib')], check=True)
            source.write_text(rust_color(self.schema) + RUST_TESTS)
            binary = folder / 'color-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
