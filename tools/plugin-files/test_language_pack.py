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

    def test_every_catalog_carries_its_own_language_for_the_quote_widget(self):
        # The quote widget picks its quotes and quotation marks by this message,
        # as a plugin does not see the UI language tag (yet).
        key = language_pack.TAG_KEY
        catalogs = sorted((data.ROOT / "tools/plugin-files/languages").glob("*.ini"))
        catalogs += sorted(data.ROOT.glob("plugins/*/i18n/*.ini"))
        carried = 0
        for catalog in catalogs:
            meta, messages = language_pack.parse(catalog)
            if catalog.parent.name == "i18n" and key not in messages:
                continue
            carried += 1
            self.assertEqual(messages.get(key), meta.get("language"), catalog)
        # English, German and every language catalog.
        self.assertGreaterEqual(carried, 3)

    def test_check_requires_the_quote_language_message(self):
        catalog = self.root / "xx.ini"
        base = catalog.read_text(encoding="utf-8")
        english = {language_pack.TAG_KEY: ("en", "de")}
        with patch.object(language_pack, "english", return_value=english):
            problems = language_pack.check("xx", [catalog])
            self.assertTrue(any(language_pack.TAG_KEY in p for p in problems), problems)
            catalog.write_text(base + f"{language_pack.TAG_KEY} = yy\n", encoding="utf-8")
            problems = language_pack.check("xx", [catalog])
            self.assertTrue(any("must be 'xx'" in p for p in problems), problems)
            catalog.write_text(base + f"{language_pack.TAG_KEY} = xx\n", encoding="utf-8")
            self.assertEqual(language_pack.check("xx", [catalog]), [])
