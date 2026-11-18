"""New languages use their catalog header and data.toml, without tool changes."""

import importlib.util
import pathlib
import tempfile
import unittest
from unittest.mock import patch

import data

SPEC = importlib.util.spec_from_file_location("language_pack", data.ROOT / "tools/language-pack.py")
language_pack = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(language_pack)


class LanguagePacks(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = pathlib.Path(temporary.name)
        self.source = self.root / "xx.ini"
        self.source.write_text(
            "[meta]\nlanguage = xx\nname = Example\nplural = one-many-other\n"
            "direction = rtl\nscript = Arab\ndigits = arab\ncalendar = islamic\n\n[messages]\n",
            encoding="utf-8",
        )
        languages = patch.object(language_pack, "LANGUAGES", self.root)
        languages.start()
        self.addCleanup(languages.stop)

    def test_merge_preserves_a_new_languages_header(self):
        chunk = self.root / "chunk.txt"
        chunk.write_text("hello = Translated\n", encoding="utf-8")
        output = self.root / "merged.ini"
        with patch.object(language_pack, "english", return_value={"hello": ("Hello", "Hallo")}):
            language_pack.merge("xx", output, [chunk])
        meta, messages = language_pack.parse(output)
        self.assertEqual(meta, language_pack.parse(self.source)[0])
        self.assertEqual(messages, {"hello": "Translated"})

    def test_merge_rejects_a_header_for_another_language(self):
        (self.root / "yy.ini").write_text(self.source.read_text(encoding="utf-8"), encoding="utf-8")
        with patch.object(language_pack, "english", return_value={}):
            with self.assertRaisesRegex(ValueError, "language = yy"):
                language_pack.merge("yy", self.root / "out.ini", [])

    def test_new_language_build_uses_its_store_entry(self):
        fields = {
            "version": "1.2.3", "formats": 4, "requires": ["script-arabic"],
            "name": "Example UI", "description": "Example menus.",
            "de": {"name": "Beispiel", "description": "Beispielmenüs."},
        }
        with patch.object(data, "plugins", return_value={"language-xx": fields}):
            with patch.object(language_pack.subprocess, "run") as compiler:
                language_pack.build("xx", self.root / "out")
        output = self.root / "out/language-xx"
        manifest = (output / "plugin.ini").read_text(encoding="utf-8")
        self.assertIn("name = Example UI\n", manifest)
        self.assertIn("version = 1.2.3\n", manifest)
        self.assertIn("requires = script-arabic\n", manifest)
        german = (output / "i18n/de.ini").read_text(encoding="utf-8")
        self.assertIn("language-xx.plugin.name = Beispiel\n", german)
        self.assertEqual(compiler.call_args.args[0][-2:], [
            str(self.source), str(output / "i18n/xx.ppcat"),
        ])

    def test_unlisted_language_needs_a_store_entry(self):
        with patch.object(data, "plugins", return_value={}):
            with self.assertRaisesRegex(SystemExit, "language-xx: add its entry"):
                language_pack.build("xx", self.root / "out")


class German(unittest.TestCase):
    """German is a pack made of every plugin's `de.ini` (#251)."""

    def test_the_store_entry_lists_every_german_catalog_as_an_input(self):
        fields = data.plugins()["language-de"]
        found = {str(f.relative_to(data.ROOT)) for f in language_pack.german_sources()}
        self.assertEqual(found, set(fields["inputs"]) - {"tools/language-pack.py", "tools/sign"})
        self.assertEqual(fields["build"], "tools/language-pack.py")
        self.assertEqual(fields["formats"], 4)

    def test_the_catalogs_merge_without_a_key_twice(self):
        files = language_pack.german_sources()
        merged = language_pack.merged_german(files)
        meta, messages = language_pack.parse(self._write(merged))
        self.assertEqual(meta["language"], "de")
        self.assertEqual(meta["name"], "Deutsch")
        self.assertEqual(
            len(messages), sum(len(language_pack.parse(f)[1]) for f in files))

    def test_a_key_two_catalogs_have_is_refused(self):
        a, b = self._write("[meta]\nlanguage = de\n[messages]\nk = A\n"), self._write(
            "[meta]\nlanguage = de\n[messages]\nk = B\n")
        with self.assertRaisesRegex(ValueError, "k: in"):
            language_pack.merged_german([a, b])

    def test_the_pack_is_built_from_the_merged_catalogs(self):
        out = pathlib.Path(tempfile.mkdtemp())
        self.addCleanup(lambda: __import__("shutil").rmtree(out, ignore_errors=True))
        with patch.object(language_pack.subprocess, "run") as compiler:
            language_pack.build("de", out)
        self.assertEqual(compiler.call_args.args[0][-1], str(out / "language-de/i18n/de.ppcat"))
        self.assertIn("id = language-de\n", (out / "language-de/plugin.ini").read_text("utf-8"))

    def _write(self, text):
        path = pathlib.Path(tempfile.mkstemp(suffix=".ini")[1])
        self.addCleanup(path.unlink)
        path.write_text(text, encoding="utf-8")
        return path
