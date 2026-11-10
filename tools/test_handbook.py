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
        self.assertNotIn(top + "src/paperpaw-sdk/native/declare/settings_value.rs", names)
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
        # And past 3.6 MB with the interface-7 round (#228): the SDK's native
        # layer (binding, entry, storage, events, registrations, declare:
        # about 14 KB zipped by 8cef08c20) and two interface-7 samples
        # (abi7-x4-min, abi7-sdk-entry: about 17 KB with their linker scripts);
        # nothing that does not belong (no tests, assets or generated tables got
        # in: checked file by file against 23256027b, 3.57 MB). The interface-7
        # work to come adds more SDK code, so the bound is 4.5 MB: room for it,
        # still a limit that notices an asset or a generated file by mistake.
        # And past 4.5 MB (4.55 MB at c82746881) with the rest of the interface-7
        # round (#228), by 0.95 MB since the 3.60 MB of d6779acc2, all of it
        # things a plugin author reads: the ABI contract docs (docs/abi, +161 KB
        # zipped: the service chapters), PaperPaw's own plugins ported to the
        # native interface (their sources are the examples: +522 KB over about
        # 20 plugins, widgets +81 KB, stats, file-manager, notes-overview,
        # keyboard, settings, mail ...), the SDK's native layer (+143 KB) and its
        # API listing (+24 KB). Checked: no test, asset or generated table is in
        # (the checks above), and the few generated source files are 31 KB zipped
        # together. About 30 plugins are still to port and add their sources, so
        # the bound is 5.5 MB; a stray asset or generated table still shows.
        # And past 5.5 MB (5.55 MB at train 9) with the plugin sources and the
        # service docs the round still adds (the ported plugins' sources are
        # what an author reads): the bound is 6.0 MB, the same kind of growth.
        # The Workshop takes no whole handbook into a prompt (it hands out topics of
        # at most MAX_TOPIC_TOKENS, src/handbook/handbook.ts), so its size has no
        # hard limit there: this keeps growth seen, a step at a time.
        self.assertLess(size, 6_000_000, "near 6 MB")


if __name__ == "__main__":
    unittest.main()
