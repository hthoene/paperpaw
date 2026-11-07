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
from abi_services import (compatible_services, load_services, op_rows, rust_services,
                          validate_services)

RUST_TESTS = r'''
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_finds_services_and_ops() {
        let echo = service_schema(core_test_echo_v1::KEY, core_test_echo_v1::MAJOR).unwrap();
        assert_eq!(echo.revision, core_test_echo_v1::REVISION);
        assert!(echo.test_only);
        let op = echo.op(core_test_echo_v1::OP_ECHO).unwrap();
        assert_eq!((op.name, op.input, op.reply), ("echo", &[1024][..], &[1024][..]));
        assert_eq!((op.max_input, op.max_reply, op.mutates), (512, 512, false));
        assert_eq!(op.bound, ServiceBound::None);
        assert!(echo.op(0).is_none() && echo.op(2).is_none());
        assert!(service_schema(core_test_echo_v1::KEY, 2).is_none());
        assert!(service_schema("core.nothing", 1).is_none());
    }
}
'''


class ServiceTests(unittest.TestCase):
    def setUp(self):
        self.schema = ABI.validate(ABI.with_services(ABI.load(ROOT / 'abi/abi.def'),
                                                     ROOT / 'abi/services'))
        self.next = self.schema['minor'] + 1

    def echo(self, schema):
        return next(s for s in schema['services'] if s['key'] == 'core.test-echo')

    def test_the_repository_schemas_load_and_emit(self):
        self.assertEqual([s['key'] for s in self.schema['services']],
                         ['core.device', 'core.document', 'core.library', 'core.lookup',
                          'core.registry',
                          'core.test-echo', 'core.ui'])
        source = rust_services(self.schema)
        for line in ['pub mod core_test_echo_v1 {', 'pub const KEY: &str = "core.test-echo";',
                     'pub const OP_ECHO: u32 = 1;', 'input: &[1024],', 'test_only: true,',
                     'pub const PAGE_MAX_ROWS: usize = 32;']:
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
            lambda s: self.echo(s).update(test_only=1),
            lambda s: self.echo(s).update(page_rows=0),
            lambda s: self.echo(s).update(page_rows=65),
            lambda s: self.echo(s).update(page_rows='32'),
            lambda s: self.echo(s).pop('test_only'),
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

    def ui(self, schema):
        return next(s for s in schema['services'] if s['key'] == 'core.ui')

    def test_row_counts_default_and_are_pinned_where_set(self):
        rows = {(service['key'], op['id']): op_rows(op, service)
                for service in self.schema['services'] for op in service['ops']}
        # core.ui (docs/abi/ui.md section 6): 16 takes 32 params, 7/14/17 are
        # unpaged lists, 1 and 3 page by 8; every other op takes one record.
        self.assertEqual(rows[('core.ui', 16)], (32, False, 0))
        self.assertEqual(rows[('core.ui', 7)], (1, False, 32))
        self.assertEqual(rows[('core.ui', 14)], (1, False, 8))
        self.assertEqual(rows[('core.ui', 17)], (1, False, 32))
        self.assertEqual(rows[('core.ui', 1)], (1, True, 8))
        self.assertEqual(rows[('core.ui', 3)], (1, True, 8))
        self.assertEqual(rows[('core.ui', 2)], (1, False, 0))
        self.assertEqual(rows[('core.registry', 1)], (1, True, 32))
        self.assertEqual(rows[('core.registry', 2)], (1, False, 0))
        self.assertEqual(rows[('core.test-echo', 1)], (1, False, 0))
        # DOC open takes 13 records; LIB ops with lib_ref and a record 2,
        # set_position 3, edit_add 4 (library.md section 1).
        self.assertEqual(rows[('core.document', 1)][0], 13)
        self.assertEqual(rows[('core.library', 14)][0], 3)
        self.assertEqual(rows[('core.library', 18)][0], 4)
        self.assertEqual(sorted(id for (key, id), row in rows.items()
                                if key == 'core.library' and row[0] == 2),
                         [4, 7, 8, 15, 16, 17, 23, 24])
        self.assertEqual(sum(1 for row in rows.values() if row[0] != 1), 12)
        source = rust_services(self.schema)
        for line in ['pub input_rows: u16,', 'pub paged: bool,', 'pub reply_rows: u16,',
                     'input_rows: 32,', 'reply_rows: 32,']:
            self.assertIn(line, source)

    def test_row_counts_are_closed(self):
        op = lambda s, id: next(o for o in self.ui(s)['ops'] if o['id'] == id)
        mutations = [
            lambda s: op(s, 16).update(input_rows=1),      # the default is left out
            lambda s: op(s, 16).update(input_rows=65),
            lambda s: op(s, 16).update(input_rows='32'),
            lambda s: op(s, 2).update(paged=False),        # no list op
            lambda s: op(s, 1).update(paged=True),         # true is the default, left out
            lambda s: op(s, 1).update(reply_rows=8),       # paged ops hold page_rows
            lambda s: op(s, 2).update(reply_rows=1),       # no list op
            lambda s: op(s, 7).pop('reply_rows'),
            lambda s: op(s, 7).update(reply_rows=0),
            lambda s: op(s, 7).update(reply_rows=65),
            lambda s: op(s, 7).update(reply=['service.end', 'service.ui_target']),
            lambda s: self.ui(s).pop('page_rows'),         # paged ops 1 and 3 need it
        ]
        for index, mutate in enumerate(mutations):
            candidate = copy.deepcopy(self.schema)
            mutate(candidate)
            with self.subTest(index=index), self.assertRaises(ValueError):
                validate_services(candidate)

    def test_module_names_are_unique(self):
        # core.test-echo and core-test.echo would share the module core_test_echo_v1.
        candidate = copy.deepcopy(self.schema)
        catalog = candidate['metadata']['services']
        entry = next(item for item in catalog if item['key'] == 'core.test-echo')
        catalog.append(dict(copy.deepcopy(entry), key='core-test.echo'))
        candidate['services'].insert(0, dict(copy.deepcopy(self.echo(candidate)),
                                             key='core-test.echo'))
        with self.assertRaisesRegex(ValueError, 'module name core_test_echo_v1 taken'):
            validate_services(candidate)

    def test_published_schemas_only_grow(self):
        grown = copy.deepcopy(self.schema)
        self.echo(grown)['revision'] = 1
        self.echo(grown)['ops'].append(dict(copy.deepcopy(self.echo(grown)['ops'][0]),
                                            id=2, name='echo_twice', since=1))
        compatible_services(self.schema, grown, self.next)
        changes = [
            lambda s: self.echo(s).update(test_only=False),
            lambda s: self.echo(s).update(since=2),
            lambda s: self.echo(s).update(page_rows=8),
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
                compatible_services(self.schema, candidate, self.next)

    def test_a_new_schema_takes_an_unreleased_minor(self):
        # A second major lives next to the first; both are pinned by (key, major).
        second = dict(copy.deepcopy(self.echo(self.schema)), major=2, since=self.next)
        grown = copy.deepcopy(self.schema)
        grown['services'].append(second)
        compatible_services(self.schema, grown, self.next)
        backdated = copy.deepcopy(grown)
        backdated['services'][-1]['since'] = self.next - 1
        with self.assertRaisesRegex(ValueError, 'backdated service schema'):
            compatible_services(self.schema, backdated, self.next)
        fresh = dict(copy.deepcopy(self.schema), services=[])
        with self.assertRaisesRegex(ValueError, 'backdated service schema'):
            compatible_services(fresh, self.schema, self.echo(self.schema)['since'] + 1)
        compatible_services(fresh, self.schema, self.echo(self.schema)['since'])

    def test_the_file_name_is_key_and_major(self):
        echo = (ROOT / 'abi/services/core.test-echo@1.def').read_text()
        with tempfile.TemporaryDirectory() as directory:
            folder = Path(directory)
            (folder / 'core.test-echo@1.def').write_text(echo)
            (folder / 'core.test-echo@2.def').write_text(echo.replace('"major": 1', '"major": 2'))
            self.assertEqual([(s['key'], s['major']) for s in load_services(folder)],
                             [('core.test-echo', 1), ('core.test-echo', 2)])
            for name in ['core.test-echo.def', 'core.test-echo@3.def', 'core.other@1.def']:
                (folder / name).write_text(echo.replace('"major": 1', '"major": 2'))
            for name in ['core.test-echo.def', 'core.test-echo@3.def', 'core.other@1.def']:
                with self.subTest(name=name):
                    single = folder / 'single'
                    single.mkdir(exist_ok=True)
                    for old in single.glob('*'):
                        old.unlink()
                    (single / name).write_text(echo.replace('"major": 1', '"major": 2'))
                    with self.assertRaises(ValueError):
                        load_services(single)

    def test_the_lock_pins_the_table(self):
        trusted = ABI.lock_record(self.schema)
        self.assertEqual(trusted['lock_format'], 11)
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
