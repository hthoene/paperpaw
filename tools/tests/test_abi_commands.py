"""Slot-15 command descriptor, generated coverage, and standalone wire vectors."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_commands', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_commands import rust_commands, validate_commands
from abi_requirements import required_minor
from abi_settings import rust_settings


RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    const FLAG: Command<'static> = Command::SetSetting {
        key: "p.f",
        value: SettingValueRef::Bool(true),
    };
    const GOLDEN: [u8; 22] = [1, 4, 19, 0, 3, b'p', b'.', b'f', 13, 1, 0, 0, 0, 1, 0, 0, 0,
                              1, 0, 0, 0, 1];

    fn decode(bytes: &[u8]) -> Result<Vec<Command<'_>>, i32> {
        commands_decode(bytes).map(|batch| batch.iter().collect())
    }

    fn encode(commands: &[Command<'_>]) -> Result<Vec<u8>, i32> {
        let mut out = std::vec![0_u8; COMMAND_MAX_BYTES];
        let len = commands_encode(commands, &mut out)?;
        out.truncate(len);
        Ok(out)
    }

    #[test]
    fn golden_vector_and_roundtrips() {
        assert_eq!(encode(&[FLAG]).unwrap(), GOLDEN);
        assert_eq!(decode(&GOLDEN).unwrap(), [FLAG]);
        let batch = commands_decode(&GOLDEN).unwrap();
        assert_eq!((batch.len(), batch.is_empty()), (1, false));
        assert!(commands_decode(&[0]).unwrap().is_empty());
        let long = "k".repeat(COMMAND_MAX_KEY_BYTES);
        let values = [SettingValueRef::Bool(false), SettingValueRef::Int(i64::MIN),
                      SettingValueRef::Int(0), SettingValueRef::Text(""),
                      SettingValueRef::Text("Grüße")];
        for value in values {
            for key in ["a", long.as_str()] {
                let command = Command::SetSetting { key, value };
                for count in 0..=COMMAND_MAX_RECORDS {
                    let commands = [command; COMMAND_MAX_RECORDS];
                    let bytes = encode(&commands[..count]).unwrap();
                    assert_eq!(decode(&bytes).unwrap(), commands[..count]);
                }
            }
        }
    }

    #[test]
    fn encoder_checks_everything_before_writing() {
        let len = GOLDEN.len();
        for cap in 0..len {
            let mut out = [0xa5; 64];
            assert_eq!(commands_encode(&[FLAG], &mut out[..cap]), Err(-3));
            assert_eq!(out, [0xa5; 64]);
        }
        let long = "k".repeat(COMMAND_MAX_KEY_BYTES + 1);
        let huge = "t".repeat(4000);
        let cases = [
            (std::vec![FLAG; COMMAND_MAX_RECORDS + 1], -3),
            (std::vec![FLAG, Command::SetSetting { key: "", value: SettingValueRef::Bool(true) }], -2),
            (std::vec![FLAG, Command::SetSetting { key: &long, value: SettingValueRef::Bool(true) }], -3),
            (std::vec![FLAG, Command::SetSetting { key: "a", value: SettingValueRef::Absent }], -2),
            (std::vec![Command::SetSetting { key: "a", value: SettingValueRef::Text(&huge) },
                       Command::SetSetting { key: "b", value: SettingValueRef::Text(&huge) }], -3),
        ];
        for (commands, error) in cases {
            let mut out = std::vec![0xa5_u8; 2 * COMMAND_MAX_BYTES];
            assert_eq!(commands_encode(&commands, &mut out), Err(error));
            assert!(out.iter().all(|&byte| byte == 0xa5));
        }
    }

    #[test]
    fn decoder_refuses_malformed_unsupported_and_oversized_batches() {
        let mut unknown_tag = GOLDEN;
        unknown_tag[1] = 5;
        let mut unknown_kind = GOLDEN;
        unknown_kind[13] = 9;
        let mut bad_text = std::vec![1, 4, 18, 0, 1, b'a', 14, 1, 0, 0, 0, 3, 0, 0, 0, 2, 0, 0, 0,
                                     0xc3, 0x28];
        bad_text[2] = (bad_text.len() - 3) as u8;
        let mut bad_key = GOLDEN;
        bad_key[5] = 0xff;
        let mut empty_key = std::vec![1, 4, 16, 0, 0, 13, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];
        empty_key[2] = (empty_key.len() - 3) as u8;
        let mut absent = std::vec![1, 4, 0, 0, 1, b'a', 12, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        absent[2] = (absent.len() - 3) as u8;
        let mut trailing = GOLDEN.to_vec();
        trailing.push(0);
        let overlong = [0x81, 0x00];
        let cases: [(&[u8], i32); 10] = [
            (&unknown_tag, -1), (&unknown_kind, -1), (&bad_text, -5), (&bad_key, -5),
            (&empty_key, -2), (&absent, -2), (&trailing, -2), (&overlong, -2),
            (&[9], -3), (&[], -2),
        ];
        for (bytes, error) in cases {
            assert_eq!(decode(bytes), Err(error), "{bytes:?}");
        }
        let mut long = std::vec![1, 4, 0x92, 0x01, 0, 0x81, 0x01];
        long.extend(core::iter::repeat_n(b'k', COMMAND_MAX_KEY_BYTES + 1));
        long.extend([13, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1]);
        assert_eq!(decode(&long), Err(-3));
        assert_eq!(decode(&std::vec![0; COMMAND_MAX_BYTES + 1]), Err(-3));
        for len in 0..GOLDEN.len() {
            assert!(decode(&GOLDEN[..len]).is_err());
        }
    }

    #[test]
    fn known_records_accept_bounded_extensions() {
        // A one-byte presence bitmap and one trailing field byte are skipped.
        let mut extended = std::vec![1, 4, 21, 1, 0xff];
        extended.extend_from_slice(&GOLDEN[4..]);
        extended.push(7);
        assert_eq!(decode(&extended).unwrap(), [FLAG]);
    }

    #[test]
    fn screen_action_redraw_and_close_commands() {
        let goldens: [(Command<'static>, &[u8]); 4] = [
            (Command::OpenScreen { screen: "a.b" }, &[1, 56, 5, 0, 3, b'a', b'.', b'b']),
            (Command::RunAction { action: "x" }, &[1, 57, 3, 0, 1, b'x']),
            (Command::Redraw { mode: 2 }, &[1, 58, 5, 0, 2, 0, 0, 0]),
            (Command::CloseScreen, &[1, 59, 1, 0]),
        ];
        for (command, bytes) in goldens {
            assert_eq!(encode(&[command]).unwrap(), bytes, "{command:?}");
            assert_eq!(decode(bytes).unwrap(), [command]);
        }
        let id = "s".repeat(COMMAND_MAX_SCREEN_BYTES);
        let mixed = [FLAG, Command::OpenScreen { screen: &id }, Command::Redraw { mode: 3 },
                     Command::RunAction { action: "p.go" }, Command::CloseScreen];
        assert_eq!(decode(&encode(&mixed).unwrap()).unwrap(), mixed);
        let long = "s".repeat(COMMAND_MAX_SCREEN_BYTES + 1);
        for (command, error) in [(Command::OpenScreen { screen: "" }, -2),
                                 (Command::RunAction { action: &long }, -3),
                                 (Command::Redraw { mode: COMMAND_REDRAW_MAX_MODE + 1 }, -2)] {
            let mut out = [0xa5_u8; 300];
            assert_eq!(commands_encode(&[FLAG, command], &mut out), Err(error));
            assert!(out.iter().all(|&byte| byte == 0xa5));
        }
        let cases: [(&[u8], i32); 5] = [
            (&[1, 56, 2, 0, 0], -2),
            (&[1, 57, 3, 0, 1, 0xff], -5),
            (&[1, 58, 5, 0, 4, 0, 0, 0], -2),
            (&[1, 58, 4, 0, 2, 0, 0], -2),
            (&[1, 60, 1, 0], -1),
        ];
        for (bytes, error) in cases {
            assert_eq!(decode(bytes), Err(error), "{bytes:?}");
        }
        // A known command skips an extended bitmap and trailing fields.
        assert_eq!(decode(&[1, 59, 3, 1, 0xff, 7]).unwrap(), [Command::CloseScreen]);
    }

    #[test]
    fn arbitrary_mutations_never_panic() {
        for index in 0..GOLDEN.len() {
            for byte in 0..=u8::MAX {
                let mut bytes = GOLDEN;
                bytes[index] = byte;
                let _result = decode(&bytes);
            }
        }
    }
}
'''


def crate(schema, tests=''):
    return ('#![no_std]\npub mod settings_wire {\n' + rust_settings(schema) +
            '}\npub mod commands_wire {\n' + rust_commands(schema) + tests + '}\n')


class CommandTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.load(ROOT / 'abi/abi.def')

    def record(self, schema):
        return next(item for item in schema['records'] if item['name'] == 'command.set_setting')

    def test_descriptor_tag_and_requirement_floor(self):
        ABI.validate(self.schema)
        self.assertEqual(self.schema['command_codec'], {'since': 1, 'max_records': 8})
        self.assertEqual(self.record(self.schema)['id'], 4)
        source = rust_commands(self.schema)
        for line in ['COMMAND_MAX_BYTES: usize = 4096;', 'COMMAND_MAX_RECORDS: usize = 8;',
                     'COMMAND_MAX_KEY_BYTES: usize = 128;', 'COMMAND_SET_SETTING_TAG: u32 = 4;']:
            self.assertIn(line, source)
        later = copy.deepcopy(self.schema)
        later['minor'] = 2
        later['command_codec']['since'] = 2
        # Every command record follows the codec it belongs to.
        for record in later['records']:
            if record['name'].startswith('command.'):
                record['since'] = 2
                for field in record['fields']:
                    field['since'] = 2
        ABI.validate(later)
        self.assertEqual(required_minor(later, [dict(kind='host', name='commands.submit')], 0), 2)
        self.assertEqual(required_minor(later, [dict(kind='host', name='heap.alloc')], 0), 1)

    def test_descriptor_and_record_are_closed(self):
        for value in [None, {}, {'since': 1, 'max_records': 8, 'extra': 0},
                      {'since': True, 'max_records': 8}, {'since': 2, 'max_records': 8},
                      {'since': 1, 'max_records': 0}, {'since': 1, 'max_records': 9}]:
            candidate = copy.deepcopy(self.schema)
            candidate['command_codec'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                ABI.validate(candidate)
        mutations = [
            lambda s: self.record(s)['fields'].reverse(),
            lambda s: self.record(s)['fields'].pop(),
            lambda s: self.record(s)['fields'][0].update(max_bytes=0),
            lambda s: self.record(s)['fields'][0].update(max_bytes=256),
            lambda s: self.record(s)['fields'][0].update(type='bytes'),
            lambda s: self.record(s)['fields'][1].update(max_bytes=4095),
            lambda s: self.record(s)['fields'][1].update(optional=True),
            lambda s: s['records'].append(dict(copy.deepcopy(self.record(s)), id=9)),
            lambda s: s['records'].remove(self.record(s)),
            lambda s: s.pop('settings_value'),
            lambda s: s['statuses'].remove(next(i for i in s['statuses']
                                                if i['name'] == 'ERR_UTF8')),
            lambda s: next(e for e in s['host_entries']
                           if e['name'] == 'commands.submit').update(max_bytes=2048),
        ]
        for index, mutate in enumerate(mutations):
            candidate = copy.deepcopy(self.schema)
            mutate(candidate)
            with self.subTest(index=index), self.assertRaises(ValueError):
                validate_commands(candidate)
        candidate = copy.deepcopy(self.schema)
        self.record(candidate)['fields'][0]['max_bytes'] = 64
        candidate['command_codec']['max_records'] = 4
        source = rust_commands(candidate)
        self.assertIn('COMMAND_MAX_KEY_BYTES: usize = 64;', source)
        self.assertIn('COMMAND_MAX_RECORDS: usize = 4;', source)
        with self.assertRaisesRegex(ValueError, 'command codec contract'):
            ABI.compatible(self.schema, candidate)

    def test_absence_backdating_and_lock_pinning(self):
        before = copy.deepcopy(self.schema)
        del before['command_codec']
        # Declaration records follow the command record, so they go with it.
        before['records'] = [item for item in before['records'] if item['id'] < 4]
        self.assertEqual(rust_commands(before), '')
        ABI.compatible(before, self.schema, 0)
        with self.assertRaisesRegex(ValueError, 'backdated'):
            ABI.compatible(before, self.schema, 1)
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['emitted_commands'], rust_commands(self.schema))
        original = ABI.rust_commands
        with patch.object(ABI, 'rust_commands',
                          lambda schema: original(schema).replace('|_| -5', '|_| -2')):
            with self.assertRaisesRegex(ValueError, 'emitted commands contract'):
                ABI.check_lock(trusted, self.schema)
        self.assertNotIn('CommandBatch', ABI.rust_module(self.schema))

    def test_generated_copies_are_checked(self):
        source = rust_commands(self.schema)
        paths = ['abi/generated/commands_wire.rs', 'crates/paperpaw-core/src/abi7/commands_wire.rs']
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

    def test_standalone_rust_no_std_and_vectors(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'commands.rs'
            source.write_text(crate(self.schema))
            subprocess.run([rustc, '--edition=2024', '--crate-type=lib', '-Dwarnings',
                            str(source), '-o', str(folder / 'commands.rlib')], check=True)
            source.write_text(crate(self.schema, RUST_TESTS))
            binary = folder / 'commands-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
