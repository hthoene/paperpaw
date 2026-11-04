"""Tests of published.py, with GitHub's list and the downloads made up.

    python3 -m unittest discover -s tools/plugin-files
"""

import hashlib
import json
import pathlib
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import published  # noqa: E402


def ppn(fingerprint, body, compiler=0x6A1F03C2, chip=5):
    """A plugin file's header with what it runs on, and a body."""
    header = bytearray(80)
    header[:4] = b"PPNP"
    struct.pack_into("<HHQ", header, 4, 2, chip, fingerprint)
    struct.pack_into("<I", header, 44, compiler)
    return bytes(header) + body


def release(tag, created, names):
    return {"tag_name": tag, "created_at": created, "draft": False,
            "assets": [{"name": n, "browser_download_url": f"https://r/{tag}/{n}"} for n in names]}


class Digests(unittest.TestCase):
    def listing(self, **more):
        asset = {"name": "a-1.0.0-x4.ppn", "size": 3, "browser_download_url": "https://r/t/a-1.0.0-x4.ppn", **more}
        return json.dumps([[{"tag_name": "t", "created_at": "2026-11-01", "draft": False, "assets": [asset]}]])

    def test_the_digest_github_gives_is_used_without_a_download(self):
        digest = hashlib.sha256(b"abc").hexdigest()
        checksum = published.checksums(self.listing(state="uploaded", digest="sha256:" + digest.upper()),
                                       fetch=lambda url: self.fail("no download"))
        self.assertEqual(checksum("a-1.0.0-x4.ppn"), digest)

    def test_without_a_digest_the_bytes_are_downloaded_once(self):
        asked = []

        def fetch(url):
            asked.append(url)
            return b"abc"

        checksum = published.checksums(self.listing(state="uploaded"), fetch=fetch)
        self.assertEqual(checksum("a-1.0.0-x4.ppn"), hashlib.sha256(b"abc").hexdigest())
        checksum("a-1.0.0-x4.ppn")
        self.assertEqual(asked, ["https://r/t/a-1.0.0-x4.ppn"])

    def test_an_asset_that_is_not_finished_is_no_published_asset(self):
        text = self.listing(state="starter")
        self.assertEqual(published.assets(text), {})
        self.assertEqual(published.partial(text), {"a-1.0.0-x4.ppn": "t"})
        self.assertEqual(published.assets(self.listing(state="uploaded")),
                         {"a-1.0.0-x4.ppn": ("t", "https://r/t/a-1.0.0-x4.ppn")})
        self.assertEqual(published.partial(self.listing(state="uploaded")), {})


class Published(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.folder = pathlib.Path(self.dir.name)
        # beta.2 had vocabulary 1.0.0, its own release 1.0.1.
        self.list = json.dumps([[
            release("vocabulary-v1.0.1", "2026-10-25", ["vocabulary-1.0.1-x4.ppn", "vocabulary-1.0.1.zip"]),
            release("v0.5.0-beta.2", "2026-10-24", ["vocabulary-1.0.0-x4.ppn", "stats-1.0.0-x4.ppn"]),
        ]])
        self.online = {
            "https://r/vocabulary-v1.0.1/vocabulary-1.0.1-x4.ppn": ppn(0xF1F6, b"published"),
            "https://r/vocabulary-v1.0.1/vocabulary-1.0.1.zip": b"folder",
        }

    def tearDown(self):
        self.dir.cleanup()

    def test_github_s_pages_and_the_first_release_of_a_name(self):
        pages = self.list + json.dumps([release("v0.5.0-beta.1", "2026-10-20", ["stats-1.0.0-x4.ppn"])])
        found = published.assets(pages)
        self.assertEqual(found["stats-1.0.0-x4.ppn"][0], "v0.5.0-beta.1")
        self.assertEqual(found["vocabulary-1.0.1-x4.ppn"][0], "vocabulary-v1.0.1")

    def test_a_published_version_for_the_same_firmware_keeps_its_bytes(self):
        built = self.folder / "vocabulary-1.0.1-x4.ppn"
        built.write_bytes(ppn(0xF1F6, b"built again, other bytes"))
        fresh = self.folder / "vocabulary-1.0.2-x4.ppn"
        fresh.write_bytes(ppn(0xF1F6, b"new"))
        said = published.files(self.folder, published.assets(self.list), self.online.__getitem__)
        self.assertEqual(built.read_bytes(), ppn(0xF1F6, b"published"))
        self.assertEqual(fresh.read_bytes(), ppn(0xF1F6, b"new"))
        self.assertEqual(said, ["vocabulary-1.0.1-x4.ppn: as published in vocabulary-v1.0.1"])

    def test_another_firmware_gets_a_new_build_of_the_version(self):
        for other in (ppn(0xBEEF, b"x"), ppn(0xF1F6, b"x", compiler=1)):
            built = self.folder / "vocabulary-1.0.1-x4.ppn"
            built.write_bytes(other)
            published.files(self.folder, published.assets(self.list), self.online.__getitem__)
            self.assertEqual(built.read_bytes(), other)

    def test_a_published_folder_must_be_the_same(self):
        folder = self.folder / "vocabulary-1.0.1.zip"
        folder.write_bytes(b"folder")
        found = published.assets(self.list)
        self.assertEqual(published.folders(self.folder, found, self.online.__getitem__), [])
        folder.write_bytes(b"another How it works")
        self.assertEqual(len(published.folders(self.folder, found, self.online.__getitem__)), 1)


if __name__ == "__main__":
    unittest.main()
