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



class BodyHashes(unittest.TestCase):
    def test_comments_and_spaces_are_no_change(self):
        self.assertEqual(shared.normal("{ a(1); // why\n  b( 2 ) }"),
                         shared.normal("{\n    a(1);\n    /* how */ b( 2 )\n}"))
        self.assertNotEqual(shared.normal("{ a(1) }"), shared.normal("{ a(2) }"))

    def test_calls_are_found_and_standard_methods_left(self):
        found = shared.called("{ let x = helper(a).height(); Self::sized::<u8>(x); v.iter().map(f) }")
        self.assertEqual(found, {"helper", "height", "sized"})

    def test_a_block_ends_past_braces_in_strings_and_comments(self):
        text = 'fn f() { let s = "}"; let c = \'}\'; // }\n g(); } tail'
        start = text.index("{")
        self.assertEqual(text[shared.block_end(text, start):], " tail")

    def test_reach_follows_the_files_own_functions_first(self):
        a, b = pathlib.Path("a.rs"), pathlib.Path("b.rs")
        functions = {
            "layout": [(a, "{ inner() }"), (b, "{ other }"), (b, "{ more }")],
            "inner": [(a, "{ 1 }")],
            "common": [(a, "{ x }"), (b, "{ y }"), (b, "{ z }")],
        }
        # `layout` is defined three times, but once in the caller's file.
        self.assertEqual(shared.reach(functions, a, "{ self.layout() }"),
                         {"{ inner() }", "{ 1 }"})
        # From another file, a name defined more than twice is not followed.
        self.assertEqual(shared.reach(functions, pathlib.Path("c.rs"), "{ common() }"), set())

    def test_generated_codec_copies_are_no_callees(self):
        import tempfile
        with tempfile.TemporaryDirectory() as root:
            crate = pathlib.Path(root)
            (crate / "src").mkdir()
            (crate / "src/pixel.rs").write_text("fn gray_levels() { 1 }\n")
            (crate / "src/wire.rs").write_text(
                shared.GENERATED + "\nfn gray_levels() { 2 }\n")
            found = shared.crate_functions(crate)
            self.assertEqual([body for _, body in found["gray_levels"]], ["{ 1 }"])

if __name__ == "__main__":
    unittest.main()
