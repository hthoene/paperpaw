"""Fragment validation, release collection and PR policy against real git history."""

import contextlib
from datetime import date
import io
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import changelog


HISTORY = "## [0.5.0] - 2026-10-01\n\n### Fixed\n\n- Old text, unchanged.\n\n[old]: https://example.org/old\n"
HEADER = "# Changelog\n\nIntroduction stays.\n\n## [Unreleased]\n\n"
DAY = date(2026, 11, 5)


class ChangelogFixture(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        (self.root / "changelog.d").mkdir()
        self.write("changelog.d/README.md", "Contributor instructions.\n")
        self.write("CHANGELOG.md", HEADER + HISTORY)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        return path

    def fragment(self, name="226-fragments.md", text="### Fixed\n\n- Fixed the reader.\n"):
        return self.write("changelog.d/" + name, text)

    def collect(self, version="0.6.0-beta.4"):
        with contextlib.redirect_stdout(io.StringIO()):
            changelog.collect(self.root, version, DAY)


class FragmentTests(ChangelogFixture):
    def test_multiple_sections_and_wrapped_bullets(self):
        self.fragment(text="### Added\n\n- A new choice.\n  More detail.\n\n  Another paragraph.\n\n### Fixed\n\n- A fix.\n")
        found = changelog.fragments(self.root)
        self.assertEqual(len(found), 1)
        self.assertEqual(found[0][1]["Added"], "- A new choice.\n  More detail.\n\n  Another paragraph.")
        self.assertEqual(found[0][1]["Fixed"], "- A fix.")

    def test_empty_malformed_and_unknown_sections_are_rejected(self):
        for text in (
            "", "\n\n", "### Fixed\n\n", "### Security\n\n- Fix.\n",
            "### fixed\n\n- Fix.\n", "## Fixed\n\n- Fix.\n",
            "### Fixed\n\n- Fix.\n\n### Fixed\n\n- Again.\n",
            "Free prose.\n", "### Fixed\n\nFree prose.\n",
            "### Fixed\n\n  Orphan continuation.\n", "### Fixed\n\n- \n",
        ):
            with self.subTest(text=text):
                self.fragment(text=text)
                with self.assertRaises(ValueError):
                    changelog.fragments(self.root)

    def test_control_characters_are_rejected(self):
        for char in ("\x00", "\x0b", "\x0c", "\x1b"):
            with self.subTest(char=repr(char)):
                self.fragment(text=f"### Fixed\n\n- Before.{char}  After.\n")
                with self.assertRaises(ValueError):
                    changelog.fragments(self.root)

    def test_filename_rules(self):
        for name in ("fix.md", "0-fix.md", "01-fix.md", "pr-0-fix.md", "226-Fix.md", "226-fix_.md", "226--fix.md", "226-fix.txt"):
            with self.subTest(name=name):
                path = self.fragment(name)
                with self.assertRaises(ValueError):
                    changelog.fragments(self.root)
                path.unlink()
        self.fragment("pr-123-a-fix.md")
        changelog.check(self.root)

    def test_order_is_numeric_then_filename(self):
        for name in ("100-last.md", "pr-10-second.md", "2-first.md", "10-first.md"):
            self.fragment(name)
        self.assertEqual(
            [path.name for path, _ in changelog.fragments(self.root)],
            ["2-first.md", "10-first.md", "pr-10-second.md", "100-last.md"],
        )

    def test_symlink_and_directory_are_rejected(self):
        path = self.root / "changelog.d/226-fix.md"
        path.symlink_to(self.root / "CHANGELOG.md")
        with self.assertRaises(ValueError):
            changelog.fragments(self.root)
        path.unlink()
        path.mkdir()
        with self.assertRaises(ValueError):
            changelog.fragments(self.root)

    def test_collection_merges_legacy_and_orders_sections_preserving_history(self):
        legacy = "### Fixed\n\n- Existing fix.\n\n### Added\n\n- Existing feature.\n\n"
        self.write("CHANGELOG.md", HEADER + legacy + HISTORY)
        self.fragment("100-last.md", "### Changed\n\n- A new workflow.\n\n### Fixed\n\n- Last fix.\n")
        self.fragment("2-first.md", "### Fixed\n\n- First fix.\n")
        self.collect()
        text = (self.root / "CHANGELOG.md").read_text(encoding="utf-8")
        self.assertTrue(text.startswith(HEADER + "## [0.6.0-beta.4] - 2026-11-05\n\n"))
        self.assertTrue(text.endswith(HISTORY))
        self.assertLess(text.index("### Added"), text.index("### Changed"))
        self.assertLess(text.index("### Changed"), text.index("### Fixed"))
        self.assertLess(text.index("- Existing fix."), text.index("- First fix."))
        self.assertLess(text.index("- First fix."), text.index("- Last fix."))
        self.assertEqual([p.name for p in (self.root / "changelog.d").iterdir()], ["README.md"])
        changelog.check(self.root, release=True)

    def test_legacy_security_section_survives(self):
        self.write("CHANGELOG.md", HEADER + "### Security\n\n- A legacy entry.\n\n" + HISTORY)
        self.collect()
        self.assertIn("### Security\n\n- A legacy entry.", (self.root / "CHANGELOG.md").read_text())

    def test_duplicate_version_and_invalid_input_leave_sources_untouched(self):
        fragment = self.fragment()
        before = (self.root / "CHANGELOG.md").read_bytes()
        for version in ("0.5.0", "v0.6.0", "0.6", "01.2.3", "1.2.3-01", "1.2.3-beta.01", "1.2.3\nwrong"):
            with self.subTest(version=version):
                with self.assertRaises(ValueError):
                    self.collect(version)
                self.assertEqual((self.root / "CHANGELOG.md").read_bytes(), before)
                self.assertTrue(fragment.exists())

    def test_malformed_fragment_leaves_changelog_and_all_fragments(self):
        good = self.fragment("2-good.md")
        bad = self.fragment("3-bad.md", "### Fixed\n\nNot a bullet.\n")
        before = (self.root / "CHANGELOG.md").read_bytes()
        with self.assertRaises(ValueError):
            self.collect()
        self.assertEqual((self.root / "CHANGELOG.md").read_bytes(), before)
        self.assertTrue(good.exists())
        self.assertTrue(bad.exists())

    def test_no_entries_cannot_create_empty_release(self):
        with self.assertRaisesRegex(ValueError, "nothing to release"):
            self.collect()

    def test_invalid_unreleased_header_is_rejected(self):
        for text in (HISTORY, HEADER + "## [Unreleased]\n\n" + HISTORY, HEADER + "Unstructured prose.\n\n" + HISTORY):
            with self.subTest(text=text):
                self.write("CHANGELOG.md", text)
                self.fragment()
                with self.assertRaises(ValueError):
                    self.collect()
                self.assertEqual((self.root / "CHANGELOG.md").read_text(), text)

    def test_replace_failure_keeps_sources_and_cleans_temporary(self):
        fragment = self.fragment()
        before = (self.root / "CHANGELOG.md").read_bytes()
        names = sorted(path.name for path in self.root.iterdir())
        with patch.object(changelog.os, "replace", side_effect=OSError("write failed")):
            with self.assertRaises(OSError):
                self.collect()
        self.assertEqual((self.root / "CHANGELOG.md").read_bytes(), before)
        self.assertTrue(fragment.exists())
        self.assertEqual(sorted(path.name for path in self.root.iterdir()), names)

    def test_release_check_rejects_pending_fragments(self):
        changelog.check(self.root, release=True)
        self.fragment()
        with self.assertRaisesRegex(ValueError, "uncollected"):
            changelog.check(self.root, release=True)


class GitPolicyTests(ChangelogFixture):
    def setUp(self):
        super().setUp()
        changelog.git(self.root, "init", "-q")
        changelog.git(self.root, "config", "user.name", "Test")
        changelog.git(self.root, "config", "user.email", "test@example.invalid")
        self.commit("Initial documentation")
        self.base = changelog.git(self.root, "rev-parse", "HEAD").strip()

    def commit(self, message):
        changelog.git(self.root, "add", "--all")
        changelog.git(self.root, "commit", "-q", "-m", message)

    def code(self, message="Change reader"):
        self.write("plugins/reader/src/new.rs", "// A user-visible change.\n")
        self.commit(message)

    def test_code_without_fragment_fails(self):
        self.code()
        with self.assertRaisesRegex(ValueError, "add a changelog.d fragment"):
            changelog.check(self.root, self.base)

    def test_new_fragment_passes_for_code(self):
        self.fragment()
        self.code()
        changelog.check(self.root, self.base)

    def test_documentation_and_fragment_only_changes_pass(self):
        self.write("docs/guide.txt", "Documentation.\n")
        self.write("CONTRIBUTING.md", "More instructions.\n")
        self.fragment()
        self.commit("Document behaviour")
        changelog.check(self.root, self.base)

    def test_modifying_an_existing_fragment_is_not_enough(self):
        self.fragment()
        self.commit("Existing fragment")
        base = changelog.git(self.root, "rev-parse", "HEAD").strip()
        self.fragment(text="### Fixed\n\n- Changed another PR's entry.\n")
        self.code()
        with self.assertRaises(ValueError):
            changelog.check(self.root, base)

    def test_explicit_head_trailer_passes(self):
        self.code("Refactor reader\n\nNo-changelog: Internal refactor; behaviour unchanged.")
        changelog.check(self.root, self.base)

    def test_body_mention_and_empty_trailer_do_not_opt_out(self):
        for message in (
            "Change reader\n\nNo-changelog: example, not an opt-out\n\nThis is further body text.",
            "Change reader\n\nNo-changelog:",
        ):
            with self.subTest(message=message):
                changelog.git(self.root, "reset", "--hard", self.base)
                self.code(message)
                with self.assertRaises(ValueError):
                    changelog.check(self.root, self.base)

    def test_an_earlier_commit_trailer_does_not_hide_later_changes(self):
        self.write("tools/internal.py", "# Internal change.\n")
        self.commit("Refactor\n\nNo-changelog: Internal only.")
        self.code()
        with self.assertRaises(ValueError):
            changelog.check(self.root, self.base)

    def test_comparison_uses_merge_base(self):
        branch = changelog.git(self.root, "symbolic-ref", "--short", "HEAD").strip()
        changelog.git(self.root, "checkout", "-q", "-b", "base-advanced")
        self.write("tools/base.py", "# Another merged change.\n")
        self.commit("Advance base")
        changelog.git(self.root, "checkout", "-q", branch)
        self.fragment()
        self.code()
        changelog.check(self.root, "base-advanced")


if __name__ == "__main__":
    unittest.main()
