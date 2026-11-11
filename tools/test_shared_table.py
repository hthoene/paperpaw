"""Tests of the rules tools/shared-table.py checks on shared.list."""

import importlib.util
import pathlib
import tempfile
import unittest
from unittest import mock

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
        # From another file, a name defined more than twice is not followed
        # where it is listed as skipped, and refused where it is not (#732).
        met = set()
        self.assertEqual(shared.reach(functions, pathlib.Path("c.rs"), "{ common() }",
                                      skipped=(frozenset({"common"}), met)), set())
        self.assertEqual(met, {"common"})
        with self.assertRaises(SystemExit):
            shared.reach(functions, pathlib.Path("c.rs"), "{ common() }")

    def test_generated_codec_copies_are_no_callees(self):
        with tempfile.TemporaryDirectory() as root:
            crate = pathlib.Path(root)
            (crate / "src").mkdir()
            (crate / "src/pixel.rs").write_text("fn gray_levels() { 1 }\n")
            (crate / "src/wire.rs").write_text(
                shared.GENERATED + "\nfn gray_levels() { 2 }\n")
            found = shared.crate_functions(crate)
            self.assertEqual([body for _, body in found["gray_levels"]], ["{ 1 }"])

    def test_a_call_defined_only_in_generated_source_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            crate = pathlib.Path(root)
            (crate / "src").mkdir()
            caller = crate / "src/caller.rs"
            caller.write_text("fn helper() { wire::decode_wire() }\n")
            wire = crate / "src/wire.rs"
            wire.write_text(shared.GENERATED + "\nfn decode_wire() { 2 }\n")
            generated = {}
            functions = shared.crate_functions(crate, generated)
            for body in ["{ wire::decode_wire() }", "{ helper() }"]:
                with self.subTest(body=body):
                    with self.assertRaisesRegex(SystemExit, "decode_wire.*generated or native source"):
                        shared.reach(functions, caller, body, generated)
            self.assertEqual(generated, {"decode_wire": [wire]})
            with mock.patch.object(shared, "bodies", return_value={
                ("test", "CODEC"): (crate, caller, "{ helper() }")
            }):
                with self.assertRaisesRegex(SystemExit, "decode_wire.*generated or native source"):
                    shared.body_hashes([("test", 1, "CODEC", "unused")])

    def test_an_unused_generated_function_is_not_parsed_or_hashed(self):
        with tempfile.TemporaryDirectory() as root:
            crate = pathlib.Path(root)
            (crate / "src").mkdir()
            caller = crate / "src/caller.rs"
            caller.write_text("fn helper() { 1 }\n")
            # Discover only the header: body parsing must not depend on which
            # Rust syntax a generated implementation happens to contain.
            (crate / "src/wire.rs").write_text(
                shared.GENERATED + '\nfn codec() { r#########"{"#########; }\n')
            generated = {}
            functions = shared.crate_functions(crate, generated)
            self.assertEqual(shared.reach(functions, caller, "{ helper() }", generated),
                             {"{ 1 }"})
            self.assertNotIn("codec", functions)

    def test_generated_names_do_not_hide_handwritten_definitions(self):
        caller, wire = pathlib.Path("caller.rs"), pathlib.Path("wire.rs")
        functions = {"decode_wire": [(caller, "{ 1 }")]}
        self.assertEqual(shared.reach(functions, caller, "{ decode_wire() }",
                                      {"decode_wire": [wire]}), {"{ 1 }"})

    def test_generated_only_calls_are_checked_before_standard_name_filtering(self):
        caller, wire = pathlib.Path("caller.rs"), pathlib.Path("wire.rs")
        with self.assertRaisesRegex(SystemExit, "len.*generated or native source"):
            shared.reach({}, caller, "{ wire::len() }", {"len": [wire]})

    def test_a_standard_name_only_the_native_facade_defines_is_the_standard_method(self):
        caller = pathlib.Path("crate/src/ui.rs")
        native = pathlib.Path("crate/src/native/stages.rs")
        self.assertEqual(shared.reach({}, caller, "{ v.retain(|x| x) }", {"retain": [native]}),
                         set())
        with self.assertRaisesRegex(SystemExit, "stage_run.*generated or native source"):
            shared.reach({}, caller, "{ stage_run() }", {"stage_run": [native]})

    def test_an_unhashed_crate_method_the_native_facade_also_names_is_that_method(self):
        caller = pathlib.Path("crate/src/ui/header.rs")
        native = pathlib.Path("crate/src/native/declare/format.rs")
        self.assertEqual(shared.reach({}, caller, '{ i18n.format("k", &[]) }',
                                      {"format": [native]}), set())
        wire = pathlib.Path("crate/src/wire.rs")
        with self.assertRaisesRegex(SystemExit, "format.*generated or native source"):
            shared.reach({}, caller, '{ i18n.format("k", &[]) }', {"format": [wire]})

    def test_the_text_inks_tone_is_that_method_beside_the_facades_ink(self):
        caller = pathlib.Path("crate/src/fonts.rs")
        native = pathlib.Path("crate/src/native/fonts.rs")
        self.assertEqual(shared.reach({}, caller, "{ let t = self.ink.tone(); }",
                                      {"tone": [native]}), set())

    def test_the_colours_luma_is_that_method_beside_the_facades_image(self):
        caller = pathlib.Path("crate/src/widgets/shape.rs")
        native = pathlib.Path("crate/src/native/image.rs")
        self.assertEqual(shared.reach({}, caller, "{ let a = paper.luma() < gray.luma(); }",
                                      {"luma": [native]}), set())

if __name__ == "__main__":
    unittest.main()
