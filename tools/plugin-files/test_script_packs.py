"""Tests of the script packs' faces (tools/script-pack.py):

    python3 -m unittest discover -s tools/plugin-files
"""

import importlib.util
import pathlib
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("script_pack", ROOT / "tools" / "script-pack.py")
script_pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(script_pack)

# The interface's font on the device: it has none of these scripts' letters.
INTERFACE = "fonts-default.source-sans"
# A pack that only adds letters to another (script-korean) and brings no
# interface face of its own: its syllables are rare, in books.
ADDS_TO_ANOTHER = {"korean-extra"}


class Faces(unittest.TestCase):
    def test_every_script_has_a_face_for_the_interfaces_font(self):
        """A book's title in the library's list is drawn in the interface's
        font, which falls back on the packs that extend it: a script whose
        pack does not shows boxes there (Greek, #387)."""
        for name, entry in script_pack.SETS.items():
            if name in ADDS_TO_ANOTHER:
                continue
            extended = [f for face in entry[1]
                        for f in (face[3] if isinstance(face[3], list) else [face[3]])]
            self.assertIn(INTERFACE, extended, name)

    def test_every_face_names_a_font_file_and_its_licence(self):
        for name, (_, faces, licence, _, _) in script_pack.SETS.items():
            for _, file, *_ in faces:
                self.assertTrue(pathlib.Path(file).exists(), (name, file))
            for file in licence if isinstance(licence, list) else [licence]:
                self.assertTrue(pathlib.Path(file).exists(), (name, file))

    def test_a_pack_that_draws_the_interface_in_another_font_says_so_in_its_licences(self):
        """Source Sans 3's licence goes with the packs drawn from it."""
        for name in ("greek", "cyrillic"):
            licences = script_pack.SETS[name][2]
            self.assertTrue(any("SourceSans3" in str(f) for f in licences), name)

    def test_face_names_are_unique_in_a_pack(self):
        for name, entry in script_pack.SETS.items():
            names = [face[0] for face in entry[1]]
            self.assertEqual(len(names), len(set(names)), name)


if __name__ == "__main__":
    unittest.main()
