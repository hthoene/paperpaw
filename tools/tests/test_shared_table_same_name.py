"""tools/shared-table.py follows a called name into at most MAX_SAME_NAME
functions of its crate. A name past that is refused unless it is listed in
SAME_NAME_SKIPPED, so a new function named like one a shared body calls
cannot drop that body out of the digest without a trace (ADR 0026; #732)."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('shared_table', ROOT / 'tools/shared-table.py')
TABLE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(TABLE)

ONE = 'pub fn measure(n: u32) -> u32 { n + 1 }\n'
TWO = 'fn measure(n: u32) -> u32 { n + 2 }\n'
THREE = 'fn measure(n: u32) -> u32 { n + 3 }\n'
BODY = '{ measure(4) }'


class SharedTableSameName(unittest.TestCase):
    def reach(self, files, skipped):
        folder = Path(tempfile.mkdtemp())
        for name, text in files.items():
            path = folder / 'src' / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding='utf-8')
        generated, users = {}, set()
        functions = TABLE.crate_functions(folder, generated, users)
        caller = folder / 'src' / 'shared.rs'
        return TABLE.reach(functions, caller, BODY, generated, users, skipped)

    def test_a_name_within_the_limit_is_followed(self):
        files = {'a.rs': ONE, 'b.rs': TWO, 'shared.rs': ''}
        reached = self.reach(files, (frozenset(), set()))
        self.assertEqual(len(reached), 2)

    def test_a_name_past_the_limit_is_refused_unless_listed(self):
        files = {'a.rs': ONE, 'b.rs': TWO, 'c.rs': THREE, 'shared.rs': ''}
        with self.assertRaises(SystemExit) as refused:
            self.reach(files, (frozenset(), set()))
        self.assertIn('SAME_NAME_SKIPPED', str(refused.exception))
        with self.assertRaises(SystemExit):
            self.reach(files, None)

    def test_a_listed_name_is_skipped_and_reported_as_met(self):
        files = {'a.rs': ONE, 'b.rs': TWO, 'c.rs': THREE, 'shared.rs': ''}
        met = set()
        self.assertEqual(self.reach(files, (frozenset({'measure'}), met)), set())
        self.assertEqual(met, {'measure'})

    def test_the_real_list_matches_the_tree(self):
        # body_hashes refuses a listed name no shared body skips any more.
        entries, _ = TABLE.parse()
        self.assertTrue(TABLE.body_hashes(entries))


if __name__ == '__main__':
    unittest.main()
