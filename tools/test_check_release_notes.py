"""Tests of check-release-notes.py.

    python3 -m unittest tools/test_check_release_notes.py
"""
import importlib.util
import pathlib
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("crn", HERE / "check-release-notes.py")
crn = importlib.util.module_from_spec(spec)
spec.loader.exec_module(crn)


class ReleaseNotes(unittest.TestCase):
    def check(self, content=None):
        with tempfile.TemporaryDirectory() as folder:
            path = pathlib.Path(folder) / "0.6.0-beta.4.md"
            if content is not None:
                path.write_bytes(content)
            return crn.problem(path)[0]

    def test_notes_within_the_limit_are_right(self):
        self.assertIsNone(self.check(b"What is new\n"))
        self.assertIsNone(self.check(b"a" * 125_000))

    def test_a_missing_empty_or_blank_file_is_wrong(self):
        self.assertIn("no ", self.check())
        self.assertIn("is empty", self.check(b""))
        self.assertIn("is empty", self.check(b" \n\t\n"))
        self.assertIn("is empty", self.check(b"\xef\xbb\xbf\n"))

    def test_over_the_limit_is_wrong(self):
        self.assertIn("at most 125000", self.check(b"a" * 125_001))

    def test_characters_count_not_bytes(self):
        self.assertIsNone(self.check("ä".encode() * 125_000))

    def test_a_file_that_is_not_utf_8_is_wrong(self):
        self.assertIn("not UTF-8", self.check(b"\xff\xfe"))


if __name__ == "__main__":
    unittest.main()
