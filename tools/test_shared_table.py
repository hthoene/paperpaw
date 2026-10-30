"""Tests of the rules tools/shared-table.py checks on shared.list."""

import importlib.util
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("shared_table", HERE / "shared-table.py")
shared = importlib.util.module_from_spec(spec)
spec.loader.exec_module(shared)

RELEASED = """\
text 1 A crates/x/src/a.rs::a
text 1 B crates/x/src/a.rs::b
fonts 1 F crates/x/src/f.rs::f
"""


def parse(text):
    return shared.parse_text(text, "test.list")


class LateAppends(unittest.TestCase):
    def problems(self, now):
        return shared.late_appends(parse(RELEASED), parse(now))

    def test_the_released_list_alone_is_fine(self):
        self.assertEqual(self.problems(RELEASED), [])

    def test_an_entry_at_the_next_revision_passes(self):
        self.assertEqual(self.problems(RELEASED + "text 2 C crates/x/src/a.rs::c\n"), [])

    def test_a_new_family_starts_at_any_revision(self):
        self.assertEqual(self.problems(RELEASED + "ui 1 U crates/x/src/u.rs::u\n"), [])

    def test_an_entry_at_the_released_revision_is_refused(self):
        # Old firmware has text at revision 1 with two entries: a plugin
        # saying "text 1" must never call a third.
        problems = self.problems(RELEASED + "text 1 C crates/x/src/a.rs::c\n")
        self.assertEqual(len(problems), 1)
        self.assertIn("text::C", problems[0])

    def test_a_bump_counts_as_released_revision(self):
        released = RELEASED + "bump text 2\n"
        now = released + "text 2 C crates/x/src/a.rs::c\n"
        self.assertEqual(len(shared.late_appends(parse(released), parse(now))), 1)

    def test_a_released_entry_changed_or_moved_is_refused(self):
        moved = "text 1 B crates/x/src/a.rs::b\ntext 1 A crates/x/src/a.rs::a\nfonts 1 F crates/x/src/f.rs::f\n"
        self.assertTrue(self.problems(moved))


class Rules(unittest.TestCase):
    def test_a_revision_below_an_earlier_line_is_refused(self):
        with self.assertRaises(SystemExit):
            parse("text 2 A p::a\ntext 1 B p::b\n")

    def test_a_batch_shares_a_revision(self):
        entries, _ = parse("text 1 A p::a\ntext 1 B p::b\n")
        self.assertEqual(len(entries), 2)


if __name__ == "__main__":
    unittest.main()
