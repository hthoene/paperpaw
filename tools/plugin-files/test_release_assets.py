"""Tests of release_assets.py: a firmware release without plugin files, and a
catalogue whose every address has an asset.

    python3 -m unittest discover -s tools/plugin-files
"""

import pathlib
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import release_assets  # noqa: E402

REPO = "hthoene/paperpaw"
STORE = f"https://github.com/{REPO}/releases/download/plugins-2026"


def entry(*urls):
    """A catalogue entry whose latest version has one build for each url."""
    lines = ['id = "news"', "", "[[versions]]", 'version = "1.0.3"', f'url = "{urls[0][0]}"', f'sha256 = "{urls[0][1]}"']
    for url, digest in urls[1:]:
        lines += ["", "[[versions.builds]]", 'device = "x4"', f'abi = "00"', f'url = "{url}"', f'sha256 = "{digest}"']
    return "\n".join(lines) + "\n"


class Names(unittest.TestCase):
    def test_a_firmware_release_has_seventy_three_assets_and_no_plugin(self):
        listed = release_assets.names("0.6.0-beta.3")
        self.assertEqual(len(listed), 73)
        self.assertEqual(len(set(listed)), 73)
        self.assertFalse([n for n in listed if release_assets.PLUGIN_FILE.match(n)])
        for name in ("paperpaw-0.6.0-beta.3-catalogue.zip", "paperpaw-0.6.0-beta.3-plugins.sha256",
                     "paperpaw-x4-pro-0.6.0-beta.3-abi.txt", "paperpaw-xtensa-std-0.6.0-beta.3.tar.gz.sig"):
            self.assertIn(name, listed)

    def test_what_a_plugin_file_is(self):
        for name in ("news-1.0.3-x4.ppn", "news-1.0.3.zip", "literata-bold-1.0.0.zip"):
            self.assertTrue(release_assets.PLUGIN_FILE.match(name), name)
        for name in ("paperpaw-0.6.0-beta.3-catalogue.zip", "paperpaw-emu-1.0-linux-x64.zip", "paperpaw-x4-1.0.bin"):
            self.assertFalse(release_assets.PLUGIN_FILE.match(name), name)


class Catalogue(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.dist = pathlib.Path(self.dir.name)
        for name in release_assets.names("0.6.0-beta.3"):
            (self.dist / name).write_bytes(b"x")

    def tearDown(self):
        self.dir.cleanup()

    def catalogue(self, text):
        with zipfile.ZipFile(self.dist / "paperpaw-0.6.0-beta.3-catalogue.zip", "w") as z:
            z.writestr("catalogue/news.toml", text)

    def wrong(self, planned, found, checksum=lambda name: "cc"):
        return release_assets.check(self.dist, "0.6.0-beta.3", REPO, planned, found, checksum)

    def test_every_address_has_an_asset(self):
        self.catalogue(entry((f"{STORE}/news-1.0.3.zip", "aa"), (f"{STORE}/news-1.0.3-x4.ppn", "bb"),
                             (f"https://github.com/{REPO}/releases/download/v0.6.0-beta.2/news-1.0.2-x4.ppn", "cc")))
        planned = {"news-1.0.3.zip": ("plugins-2026", "aa"), "news-1.0.3-x4.ppn": ("plugins-2026", "bb")}
        found = {"news-1.0.2-x4.ppn": ("v0.6.0-beta.2", f"https://github.com/{REPO}/releases/download/v0.6.0-beta.2/news-1.0.2-x4.ppn")}
        self.assertEqual(self.wrong(planned, found), [])

    def test_a_published_file_with_other_bytes_than_the_entry_s_is_caught(self):
        old = f"https://github.com/{REPO}/releases/download/v0.6.0-beta.2/news-1.0.2-x4.ppn"
        self.catalogue(entry((f"{STORE}/news-1.0.3.zip", "aa"), (old, "cc")))
        found = {"news-1.0.2-x4.ppn": ("v0.6.0-beta.2", old)}
        planned = {"news-1.0.3.zip": ("plugins-2026", "aa")}
        self.assertEqual(self.wrong(planned, found, lambda name: "cc"), [])
        wrong = self.wrong(planned, found, lambda name: "dd")
        self.assertEqual(len(wrong), 1)
        self.assertIn("other bytes", wrong[0])
        self.assertIn("raise the plugin's version", wrong[0])

    def test_an_address_without_an_asset_is_caught(self):
        self.catalogue(entry((f"{STORE}/news-1.0.3.zip", "aa"), (f"{STORE}/news-1.0.3-x4-pro.ppn", "bb")))
        wrong = self.wrong({"news-1.0.3.zip": ("plugins-2026", "aa")}, {})
        self.assertEqual(len(wrong), 1)
        self.assertIn("news-1.0.3-x4-pro.ppn has no asset", wrong[0])

    def test_a_checksum_that_is_not_the_planned_file_s_is_caught(self):
        self.catalogue(entry((f"{STORE}/news-1.0.3.zip", "zz")))
        wrong = self.wrong({"news-1.0.3.zip": ("plugins-2026", "aa")}, {})
        self.assertIn("checksum", wrong[0])

    def test_a_file_planned_for_another_release_is_caught(self):
        self.catalogue(entry((f"{STORE}/news-1.0.3.zip", "aa")))
        wrong = self.wrong({"news-1.0.3.zip": ("plugins-2026-2", "aa")}, {})
        self.assertIn("planned for plugins-2026-2", wrong[0])

    def test_an_address_outside_the_repository_is_caught(self):
        self.catalogue(entry(("https://example.invalid/news-1.0.3.zip", "aa")))
        self.assertIn("is not under", self.wrong({}, {})[0])

    def test_a_missing_firmware_file_and_a_plugin_file_in_the_list(self):
        self.catalogue(entry((f"{STORE}/news-1.0.3.zip", "aa")))
        (self.dist / "paperpaw-x3-0.6.0-beta.3-abi.txt").unlink()
        wrong = self.wrong({"news-1.0.3.zip": ("plugins-2026", "aa")}, {})
        self.assertEqual(wrong, [f"paperpaw-x3-0.6.0-beta.3-abi.txt is not in {self.dist}"])


if __name__ == "__main__":
    unittest.main()
