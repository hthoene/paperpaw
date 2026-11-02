"""Tests of the UI language packs (data.toml's `language-*`, ADR 0033): every
message translated, the placeholders as English's, the pack described in both
languages, the letters pack they need, the digits and direction of the language:

    python3 -m unittest discover -s tools/plugin-files
"""

import importlib.util
import pathlib
import sys
import unittest

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import data  # noqa: E402

spec = importlib.util.spec_from_file_location("language_pack", data.ROOT / "tools/language-pack.py")
pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pack)

# Packs whose catalog is complete: every English message has its translation.
COMPLETE = {"fa": "1.0.0", "ur": "1.0.0"}
# The pack with the letters these interfaces are drawn in (#97): without it
# the screens show boxes, so the pack requires it and its description says so.
LETTERS = "script-arabic"
# Letters of the Arabic alphabet that Persian and Urdu write otherwise (the
# Arabic yeh and kaf; Urdu's heh goal and heh doachashmee are ہ ھ).
ARABIC_ONLY = "يك"


class LanguagePacks(unittest.TestCase):
    def test_each_is_offered_and_described(self):
        listed = data.plugins()
        for lang, version in COMPLETE.items():
            fields = listed[f"language-{lang}"]
            self.assertEqual(fields["version"], version, lang)
            self.assertEqual(fields["build"], "tools/language-pack.py")
            self.assertTrue(fields["off_by_default"])
            self.assertEqual(fields["formats"], 4, "the .ppcat catalogs need formats 4")
            for key in ["name", "description"]:
                self.assertTrue(fields[key] and fields["de"][key], (lang, key))
            for path in [fields["build"], *fields["inputs"]]:
                self.assertTrue((data.ROOT / path).exists(), (lang, path))
            self.assertIn(lang, pack.META)
            self.assertEqual(fields["requires"], [LETTERS], lang)
            self.assertIn(f"requires = {LETTERS}\n", data.plugin_ini(f"language-{lang}", fields))
            self.assertIn("Needs Arabic letters", fields["description"], lang)
            self.assertIn("Braucht die Arabische Schrift", fields["de"]["description"], lang)
            self.assertIn(LETTERS, listed, lang)
            self.assertNotIn("achine", fields["description"], "no machine-made mark (owner)")

    def test_the_catalog_has_every_message_of_english_and_no_other(self):
        english = pack.english()
        for lang in COMPLETE:
            path = pack.LANGUAGES / f"{lang}.ini"
            self.assertEqual(pack.check(lang, [path]), [], lang)
            missing = [k for k in english if k not in pack.parse(path)[1]]
            self.assertEqual(missing[:5], [], f"{lang}: {len(missing)} messages of English's missing")

    def test_the_meta_says_direction_script_digits_and_plural(self):
        want = {
            "fa": {"plural": "zero-one-other", "digits": "arabext"},
            "ur": {"plural": "one-other", "digits": "arabext"},
        }
        for lang, extra in want.items():
            meta = pack.parse(pack.LANGUAGES / f"{lang}.ini")[0]
            self.assertEqual(meta["language"], lang)
            self.assertEqual(meta["direction"], "rtl")
            self.assertEqual(meta["script"], "Arab")
            for key, value in extra.items():
                self.assertEqual(meta[key], value, (lang, key))
            self.assertEqual(meta, pack.META[lang]["meta"], lang)

    def test_the_plural_pairs_have_both_forms_and_the_letters_are_the_language_s(self):
        for lang in COMPLETE:
            messages = pack.parse(pack.LANGUAGES / f"{lang}.ini")[1]
            for key in messages:
                if key.endswith(".one"):
                    self.assertIn(key[: -len(".one")] + ".other", messages, key)
            text = "".join(messages.values())
            for letter in ARABIC_ONLY:
                self.assertNotIn(letter, text, f"{lang}: Arabic {letter!r} for a Persian/Urdu letter")


if __name__ == "__main__":
    unittest.main()
