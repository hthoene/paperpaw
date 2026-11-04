"""Tests of data.toml's script packs, the ones offered in the store:

    python3 -m unittest discover -s tools/plugin-files
"""

import pathlib
import sys
import tomllib
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import data  # noqa: E402

# The versions they are at now: a change of a pack bumps it here too.
PACKS = {
    "script-arabic": "1.1.2",
    "script-hebrew": "1.1.2",
    "script-greek": "1.0.2",
    "script-cyrillic": "1.0.2",
    "script-korean": "1.1.3",
    "script-korean-extra": "1.0.2",
}


def ready():
    """The Korean entries (they were once commented out until the owner
    decided): all of data.toml's."""
    return data.plugins()


class ScriptPacks(unittest.TestCase):
    def test_they_are_offered(self):
        self.assertTrue(set(PACKS) <= set(data.plugins()))

    def test_each_is_complete(self):
        listed = data.plugins()
        for pack, version in PACKS.items():
            fields = listed[pack]
            self.assertEqual(fields["version"], version, pack)
            self.assertEqual(fields["build"], "tools/script-pack.py")
            self.assertTrue(fields["off_by_default"])
            self.assertEqual(fields["licence"], "OFL-1.1")
            # Its name and description in both languages, its files there.
            for key in ["name", "description"]:
                self.assertTrue(fields[key] and fields["de"][key], (pack, key))
            for path in [fields["build"], *fields["inputs"]]:
                self.assertTrue((data.ROOT / path).exists(), (pack, path))
            # What the pack builder writes for it.
            self.assertIn(f"id = {pack}\n", data.plugin_ini(pack, fields))
            # A pack with a keyboard (points/) is made for formats 3, else
            # PaperPaw leaves the keyboard out.
            if any("/keyboards/" in path for path in fields["inputs"]):
                self.assertGreaterEqual(fields.get("formats", 1), 3, pack)
                self.assertIn("formats = ", data.plugin_ini(pack, fields), pack)


class Korean(unittest.TestCase):
    def test_the_korean_pack_is_ready(self):
        fields = ready()["script-korean"]
        self.assertEqual(fields["version"], "1.1.3")
        self.assertEqual(fields["build"], "tools/script-pack.py")
        self.assertTrue(fields["off_by_default"])
        self.assertEqual(fields["licence"], "OFL-1.1")
        for key in ["name", "description"]:
            self.assertTrue(fields[key] and fields["de"][key])
        # Its keyboard is among what its build reads.
        self.assertIn("tools/plugin-files/keyboards/korean.txt", fields["inputs"])
        for path in [fields["build"], *fields["inputs"]]:
            self.assertTrue((data.ROOT / path).exists(), path)
        self.assertIn("id = script-korean\n", data.plugin_ini("script-korean", fields))
        # The font's licence reserves its name: the store's texts do not
        # use it (NOTICE).
        texts = [fields["name"], fields["description"], *fields["de"].values()]
        self.assertFalse([t for t in texts if "Nanum" in t])

    def test_the_extra_pack_adds_what_the_korean_pack_lacks(self):
        entries = ready()
        fields = entries["script-korean-extra"]
        self.assertEqual(fields["build"], "tools/script-pack.py")
        self.assertTrue(fields["off_by_default"])
        self.assertEqual(fields["licence"], "OFL-1.1")
        # Names, descriptions and how it works, in both languages; the
        # size is named where the store shows it.
        for key in ["name", "description", "how"]:
            self.assertTrue(fields[key] and fields["de"][key], key)
        self.assertIn("30 MiB", fields["description"])
        self.assertIn("30 MiB", fields["de"]["description"])
        for path in [fields["build"], *fields["inputs"]]:
            self.assertTrue((data.ROOT / path).exists(), path)
        self.assertIn("id = script-korean-extra\n",
                      data.plugin_ini("script-korean-extra", fields))
        catalog = dict(data.catalogs("script-korean-extra", fields))
        for lang in ["en", "de"]:
            self.assertIn("script-korean-extra.plugin.how = \"", catalog[f"i18n/{lang}.ini"])
            self.assertNotIn("\n\n\n", catalog[f"i18n/{lang}.ini"])
        # Its letters and the Korean pack's are all the syllables, none twice.
        base, rest = sets("hangul-ks.txt"), sets("hangul-rest.txt")
        self.assertFalse(base & rest)
        self.assertEqual(base | rest, set(range(0xAC00, 0xD7A4)))
        self.assertEqual((len(base), len(rest)), (2350, 8822))


def sets(name):
    """The code points of a character set file of tools/fontgen/sets."""
    found = set()
    text = (data.ROOT / "tools/fontgen/sets" / name).read_text(encoding="utf-8")
    for line in text.splitlines():
        for item in line.split("#")[0].split():
            first, _, last = item.removeprefix("U+").partition("-")
            found.update(range(int(first, 16), int(last or first, 16) + 1))
    return found


if __name__ == "__main__":
    unittest.main()


class Requires(unittest.TestCase):
    """The packs that bring no letters of their own name the pack that does,
    so the store can fetch it with them (#97)."""

    def test_the_arabic_interface_needs_the_arabic_letters(self):
        fields = data.plugins()["language-ar"]
        self.assertEqual(fields["requires"], ["script-arabic"])
        self.assertIn("requires = script-arabic\n", data.plugin_ini("language-ar", fields))
        # Said where a reader looks first, and in German too.
        self.assertIn("Needs Arabic letters", fields["description"])
        self.assertIn("Braucht die Arabische Schrift", fields["de"]["description"])
        self.assertIn("Arabic letters", fields["how"])

    def test_the_rare_hangul_need_the_korean_letters(self):
        fields = data.plugins()["script-korean-extra"]
        self.assertEqual(fields["requires"], ["script-korean"])
        self.assertIn("requires = script-korean\n", data.plugin_ini("script-korean-extra", fields))

    def test_every_requirement_is_a_pack_of_data_toml_and_needs_formats_4(self):
        entries = data.plugins()
        for pack, fields in entries.items():
            for needed in fields.get("requires", []):
                self.assertIn(needed, entries, (pack, needed))
                self.assertNotEqual(needed, pack)
            if fields.get("requires"):
                self.assertGreaterEqual(fields.get("formats", 1), 4, pack)

    def test_requires_before_formats_4_is_refused(self):
        fields = dict(data.plugins()["script-korean-extra"], formats=3)
        with self.assertRaises(ValueError):
            data.plugin_ini("script-korean-extra", fields)
