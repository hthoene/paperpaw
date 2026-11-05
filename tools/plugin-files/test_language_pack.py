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

    def test_regional_language_keeps_its_tag_but_uses_a_lowercase_plugin_id(self):
        source = self.root / "pt-br.ini"
        source.write_text(
            "[meta]\nlanguage = pt-BR\nname = Português (Brasil)\n"
            "plural = zero-one-many-other\n\n[messages]\n", encoding="utf-8",
        )
        fields = {
            "version": "1.0.0", "formats": 4,
            "build": "tools/language-pack.py",
            "inputs": ["tools/plugin-files/languages/pt-br.ini"],
            "name": "Brazilian Portuguese UI", "description": "Brazilian menus.",
            "de": {"name": "Brasilianisches Portugiesisch", "description": "Menüs."},
        }
        with patch.object(data, "plugins", return_value={"language-pt-br": fields}):
            with patch.object(language_pack.subprocess, "run") as compiler:
                language_pack.build("pt-br", self.root / "out")
        output = self.root / "out/language-pt-br"
        self.assertIn("id = language-pt-br\n", (output / "plugin.ini").read_text())
        self.assertEqual(compiler.call_args.args[0][-2:], [
            str(source), str(output / "i18n/pt-br.ppcat"),
        ])
        # The release driver uses the source tag declared in the inputs.
        with patch.object(data, "plugins", return_value={"language-pt-br": fields}):
            with patch.object(language_pack.subprocess, "run") as compiler:
                language_pack.main([str(self.root / "release")])
        self.assertEqual(compiler.call_args.args[0][-2:], [
            str(source), str(self.root / "release/language-pt-br/i18n/pt-br.ppcat"),
        ])

    def test_merge_keeps_regional_metadata_in_a_lowercase_source(self):
        source = self.root / "pt-br.ini"
        source.write_text(
            "[meta]\nlanguage = pt-BR\nname = Português (Brasil)\n"
            "plural = zero-one-many-other\n\n[messages]\n", encoding="utf-8",
        )
        chunk = self.root / "chunk.txt"
        chunk.write_text("hello = Olá\n", encoding="utf-8")
        output = self.root / "merged.ini"
        with patch.object(language_pack, "english", return_value={"hello": ("Hello", "Hallo")}):
            language_pack.merge("pt-br", output, [chunk])
        meta, messages = language_pack.parse(output)
        self.assertEqual(meta, language_pack.parse(source)[0])
        self.assertEqual(messages, {"hello": "Olá"})
