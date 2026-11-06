"""Tests of handbook.py, on this checkout:

    python3 -m unittest discover -s tools -p "test_handbook.py"
"""

import contextlib
import io
import pathlib
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import handbook  # noqa: E402


class Handbook(unittest.TestCase):
    def test_tests_at_a_file_s_end_are_left_out(self):
        code = "fn a() {}\n"
        tests = "#[cfg(test)]\nmod tests {\n    fn b() { let _ = '{'; }\n}\n"
        self.assertEqual(handbook.without_tests(code + "\n" + tests), code + "\n")
        more = "\n#[cfg(test)]\nmod hostile_tests;\n"
        self.assertEqual(handbook.without_tests(code + "\n" + tests + more), code + "\n")
        # Where something follows that is no test, the file stays whole.
        after = code + "\n" + tests + "\nfn c() {}\n"
        self.assertEqual(handbook.without_tests(after), after)
        self.assertEqual(handbook.without_tests(code), code)

    def test_it_holds_the_sdk_and_the_plugins_without_tests_or_assets(self):
        with tempfile.TemporaryDirectory() as out:
            registry = pathlib.Path(out, "registry.json")
            registry.write_text("{}\n", encoding="utf-8")
            book = pathlib.Path(out, "handbook.zip")
            with contextlib.redirect_stdout(io.StringIO()):
                handbook.main(str(registry), "test", str(book))
            with zipfile.ZipFile(book) as z:
                names = z.namelist()
                readme = z.read("paperpaw-handbook-test/README.md").decode("utf-8")
                battery = z.read("paperpaw-handbook-test/plugins/battery/src/lib.rs").decode("utf-8")
            size = book.stat().st_size
        top = "paperpaw-handbook-test/"
        for name in ["api/paperpaw-sdk.txt", "src/paperpaw-sdk/lib.rs", "docs/plugins.md",
                     "plugins/battery/Cargo.toml", "plugins/battery/how/en.txt",
                     "plugins/battery/i18n/en.ini"]:
            self.assertIn(top + name, names)
        for name in names:
            parts = pathlib.PurePosixPath(name).parts
            self.assertFalse({"tests", "testdata", "snapshots", "target"} & set(parts), name)
            self.assertNotIn(pathlib.PurePosixPath(name).suffix, handbook.ASSETS, name)
            self.assertFalse(name.endswith(("tests.rs", "_tests.rs")), name)
        # No design proposal in draft; the docs they become are in (docs/abi/*.md).
        self.assertFalse(any(n.startswith(top + "docs/abi/proposals/") for n in names))
        self.assertIn(top + "docs/abi/records.md", names)
        # Nor the generated table of icon names: data with nothing to read, 53 KB zipped.
        self.assertNotIn(top + "src/paperpaw-sdk/icon_table.rs", names)
        self.assertIn(top + "src/paperpaw-sdk/icons.rs", names)
        # Nor the SDK's private copies of the generated wire codecs.
        self.assertFalse(any(n.startswith(top + "src/paperpaw-sdk/native/wire/") for n in names))
        self.assertNotIn(top + "src/paperpaw-sdk/native/declare/records_wire.rs", names)
        # Neither the kernel nor a shell.
        self.assertFalse(any(n.startswith((top + "src/paperpaw-core/", top + "shells/")) for n in names))
        self.assertNotIn("#[cfg(test)]\nmod tests {", battery)
        for part in ["what a plugin may use", "examples of good solutions", "left out on"]:
            self.assertIn(part, readme)
        # Grown past 3 MB with the plugins' docs and sources (flash cards, store),
        # and by 0.1 MB with the Mail plugin's sources (3.2 to 3.3 MB), and by
        # 0.1 MB with the sorted library and the TLS sockets (3.4 to 3.5 MB), and
        # past 3.5 MB with the picture home (#205) and the widget colours (#206).
        # The interface-7 draft proposals (0.11 MB of text, 45 KB zipped) are left
        # out: working papers, not for plugin authors. Without them the handbook
        # of abi7/integration was 3.60 MB at 12edb5ab; without the generated table
        # of icon names (icon_table.rs, 53 KB zipped) it is 3.54 MB.
        # The Workshop takes no whole handbook into a prompt (it hands out topics of
        # at most MAX_TOPIC_TOKENS, src/handbook/handbook.ts), so its size has no
        # hard limit there: this keeps growth seen, a step at a time.
        # Past 3.6 MB with interface 7's SDK (#228: the flat C boundary's typed
        # wrappers, records and their docs), even with the private copies of the
        # generated wire codecs left out (about 20 KB zipped): 3.60 MB at #233.
        self.assertLess(size, 3_700_000, "near 3.7 MB")


if __name__ == "__main__":
    unittest.main()
