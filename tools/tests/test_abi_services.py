"""Service schemas (`abi/services/*.def`): validation, growth rules, emitted table."""
import copy
import importlib.util
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('abi_gen_services', ROOT / 'tools/abi-gen.py')
ABI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ABI)
from abi_services import compatible_services, rust_services, validate_services

RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_finds_services_and_ops() {
        let echo = service_schema(core_test_echo::KEY, core_test_echo::MAJOR).unwrap();
        assert_eq!(echo.revision, core_test_echo::REVISION);
        let op = echo.op(core_test_echo::OP_ECHO).unwrap();
        assert_eq!((op.name, op.input, op.reply), ("echo", &[1024][..], &[1024][..]));
        assert_eq!((op.max_input, op.max_reply, op.mutates), (512, 512, false));
        assert_eq!(op.bound, ServiceBound::None);
        assert!(echo.op(0).is_none() && echo.op(2).is_none());
        assert!(service_schema(core_test_echo::KEY, 2).is_none());
        assert!(service_schema("core.nothing", 1).is_none());
    }
}
'''


class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.validate(ABI.with_services(ABI.load(ROOT / 'abi/abi.def'),
                                                     ROOT / 'abi/services'))

    def echo(self, schema):
        return next(s for s in schema['services'] if s['key'] == 'core.test-echo')

    def test_the_repository_schemas_load_and_emit(self):
        self.assertEqual([s['key'] for s in self.schema['services']], ['core.test-echo'])
        source = rust_services(self.schema)
        for line in ['pub const KEY: &str = "core.test-echo";', 'pub const OP_ECHO: u32 = 1;',
                     'input: &[1024],']:
            self.assertIn(line, source)
        for path in ['abi/generated/services_wire.rs',
                     'crates/paperpaw-core/src/abi7/services_wire.rs',
                     'crates/paperpaw-sdk/src/native/wire/services_wire.rs']:
            self.assertEqual((ROOT / path).read_text(), source)
        self.assertEqual(ABI.lock_record(self.schema)['emitted_services'], source)
        without = copy.deepcopy(self.schema)
        del without['services']
        self.assertEqual(rust_services(without), '')

    def test_schemas_are_closed(self):
        op = lambda s: self.echo(s)['ops'][0]
        mutations = [
            lambda s: self.echo(s).update(key='Core.echo'),
            lambda s: self.echo(s).update(major=0),
            lambda s: self.echo(s).update(major=2),        # no catalog entry
            lambda s: self.echo(s).update(revision=1),     # catalog says 0
            lambda s: self.echo(s).update(since=99),
            lambda s: self.echo(s).update(extra=1),
            lambda s: self.echo(s).update(ops=[]),
            lambda s: op(s).update(id=0),
            lambda s: op(s).update(name='Echo'),
            lambda s: op(s).update(since=1),
            lambda s: op(s).update(effect='consume'),
            lambda s: op(s).update(bound='radio'),
            lambda s: op(s).update(feature='no.such-feature'),
            lambda s: op(s).update(input=['event.tick']),
            lambda s: op(s).update(reply=['service.nothing']),
            lambda s: op(s).update(input=['service.echo', 'service.echo']),
            lambda s: op(s).update(max_input=0),
            lambda s: op(s).update(max_reply=4097),
            lambda s: self.echo(s)['ops'].append(copy.deepcopy(op(s))),
            lambda s: self.echo(s)['ops'].append(dict(copy.deepcopy(op(s)), id=2)),
            lambda s: s['services'].append(copy.deepcopy(self.echo(s))),
        ]
        for index, mutate in enumerate(mutations):
            candidate = copy.deepcopy(self.schema)
            mutate(candidate)
            with self.subTest(index=index), self.assertRaises(ValueError):
                validate_services(candidate)

    def test_published_schemas_only_grow(self):
        grown = copy.deepcopy(self.schema)
        self.echo(grown)['revision'] = 1
        self.echo(grown)['ops'].append(dict(copy.deepcopy(self.echo(grown)['ops'][0]),
                                            id=2, name='echo_twice', since=1))
        compatible_services(self.schema, grown)
        changes = [
            lambda s: self.echo(s)['ops'][0].update(max_reply=256),
            lambda s: self.echo(s)['ops'][0].update(name='echoes'),
            lambda s: self.echo(s)['ops'].pop(),
            lambda s: s.update(services=[]),
            lambda s: self.echo(s).update(major=2),
            lambda s: self.echo(s)['ops'].append(dict(copy.deepcopy(self.echo(s)['ops'][0]),
                                                      id=2, name='late', since=0)),
        ]
        for index, change in enumerate(changes):
            candidate = copy.deepcopy(self.schema)
            change(candidate)
            with self.subTest(index=index), self.assertRaises(ValueError):
                compatible_services(self.schema, candidate)

    def test_the_lock_pins_the_table(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['lock_format'], 9)
        ABI.check_lock(trusted, self.schema)
        changed = copy.deepcopy(self.schema)
        self.echo(changed)['ops'][0]['max_reply'] = 256
        with self.assertRaises(ValueError):
            ABI.check_lock(trusted, changed)

    def test_standalone_rust_no_std(self):
        rustc = shutil.which('rustc') or str(Path.home() / '.cargo/bin/rustc')
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            source = folder / 'services.rs'
            source.write_text('#![no_std]\n' + rust_services(self.schema) + RUST_TESTS)
            binary = folder / 'services-tests'
            subprocess.run([rustc, '--edition=2024', '--test', '-Dwarnings', str(source),
                            '-o', str(binary)], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    unittest.main()
