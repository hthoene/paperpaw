"""tools/shared-table.py never hashes the native facade, so it refuses a
shared body that could reach it (ADR 0026; #228)."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('shared_table', ROOT / 'tools/shared-table.py')
TABLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TABLE)

NATIVE = '''
pub fn is_last(n: u32) -> bool { n == 0 }
pub fn only_native(n: u32) -> u32 { n + 1 }
'''
OTHER = '''
pub fn is_last(n: u32) -> bool { n == 9 }
'''


class SharedTableNative(unittest.TestCase):
    def crate(self, files):
        folder = Path(tempfile.mkdtemp())
        for name, text in files.items():
            path = folder / 'src' / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding='utf-8')
        return folder

    def reach(self, folder, caller, body):
        generated, users = {}, set()
        functions = TABLE.crate_functions(folder, generated, users)
        return TABLE.reach(functions, folder / 'src' / caller, body, generated, users)

    def test_native_functions_are_indexed_by_name_only(self):
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': OTHER, 'ui.rs': ''})
        generated, users = {}, set()
        functions = TABLE.crate_functions(folder, generated, users)
        self.assertEqual([path.name for path, _ in functions['is_last']], ['other.rs'])
        self.assertIn('only_native', generated)
        self.assertNotIn('only_native', functions)
        self.assertEqual(users, set())

    def test_a_body_without_native_reaches_the_other_function(self):
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': OTHER,
                             'ui.rs': 'use crate::other::is_last;\n'})
        reached = self.reach(folder, 'ui.rs', '{ is_last(3) }')
        self.assertEqual(reached, {TABLE.normal('{ n == 9 }')})

    def test_a_native_path_is_refused(self):
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': OTHER, 'ui.rs': ''})
        for body in ('{ crate::native::is_last(3) }', '{ native :: only_native(3) }'):
            with self.assertRaises(SystemExit) as refused:
                self.reach(folder, 'ui.rs', body)
            self.assertIn('native:: path', str(refused.exception))

    def test_a_native_path_in_a_reached_function_is_refused(self):
        other = 'pub fn helper() -> bool { crate::native::is_last(1) }\n'
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': other, 'ui.rs': ''})
        with self.assertRaises(SystemExit) as refused:
            self.reach(folder, 'ui.rs', '{ helper() }')
        self.assertIn('other.rs', str(refused.exception))

    def test_a_name_imported_from_native_is_refused_even_if_defined_elsewhere(self):
        ui = 'use crate::native::is_last;\n'
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': OTHER, 'ui.rs': ui})
        with self.assertRaises(SystemExit) as refused:
            self.reach(folder, 'ui.rs', '{ is_last(3) }')
        self.assertIn('is_last', str(refused.exception))
        self.assertIn('imports from native', str(refused.exception))

    def test_a_grouped_import_from_native_counts(self):
        ui = 'use crate::{\n    native::{only_native},\n    other,\n};\n'
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': OTHER, 'ui.rs': ui})
        with self.assertRaises(SystemExit):
            self.reach(folder, 'ui.rs', '{ only_native(3) }')

    def test_a_name_only_native_defines_is_refused(self):
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': OTHER, 'ui.rs': ''})
        with self.assertRaises(SystemExit) as refused:
            self.reach(folder, 'ui.rs', '{ only_native(3) }')
        self.assertIn('generated or native source', str(refused.exception))

    def test_the_other_crate_named_native_is_not_the_facade(self):
        folder = self.crate({'native/mod.rs': NATIVE, 'other.rs': OTHER,
                             'ui.rs': 'use paperpaw_native::Api;\n'})
        reached = self.reach(folder, 'ui.rs', '{ paperpaw_native::size(); is_last(3) }')
        self.assertEqual(reached, {TABLE.normal('{ n == 9 }')})


if __name__ == '__main__':
    unittest.main()
