"""The release job's plugin path, end to end on made-up files (the steps of
release.yml's dry run): the store release is chosen, the catalogue is written
with the store release's addresses, the upload is planned for what was not
published, and the lists are checked.

    python3 -m unittest discover -s tools/plugin-files
"""

import contextlib
import hashlib
import io
import json
import pathlib
import struct
import sys
import tempfile
import unittest
import unittest.mock
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import catalogue  # noqa: E402
import release_assets  # noqa: E402
import store_release  # noqa: E402

REPO = "hthoene/paperpaw"
VERSION = "0.6.0-beta.3"
KEY = bytes(range(32))


def signed(fingerprint, chip, body=b""):
    header = b"PPNP" + bytes(4) + struct.pack("<Q", fingerprint)
    header += bytes(80 - len(header))
    trailer = b"PPNS" + bytes(8) + KEY
    return header + body + trailer + bytes(catalogue.SIGNATURE_LEN - len(trailer))


def release(tag, created, assets):
    """`assets`: (name, size) or (name, size, sha256 of its bytes)."""
    return {"tag_name": tag, "created_at": created, "draft": False, "assets": [
        {"name": a[0], "size": a[1], "state": "uploaded",
         "digest": "sha256:" + (a[2] if len(a) > 2 else hashlib.sha256(b"x").hexdigest()),
         "browser_download_url": f"https://github.com/{REPO}/releases/download/{tag}/{a[0]}"}
        for a in assets]}


def checksum_of(listing):
    return store_release.published.checksums(listing.read_text(encoding="utf-8"))


class Flow(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.dist = self.root / "dist"
        self.plugins = self.dist / "plugins"
        self.plugins.mkdir(parents=True)
        self.ids = [p for p, _ in catalogue.plugins()]
        for plugin in self.ids:
            version = catalogue.plugin_version(plugin)
            for device in catalogue.DEVICES:
                (self.plugins / f"{plugin}-{version}-{device}.ppn").write_bytes(signed(0x85073A61323717FF, 5, device.encode()))
        for name in release_assets.names(VERSION):
            (self.dist / name).write_bytes(b"x")

    def run_catalogue(self, releases):
        listing = self.root / "releases.json"
        listing.write_text(json.dumps([releases]))
        incoming = store_release.upper_bound(self.plugins)
        tag, _ = store_release.choose(store_release.load(listing), 2026, incoming)
        argv = ["catalogue.py", str(self.plugins), VERSION, f"https://github.com/{REPO}/releases/download/{tag}",
                "--date", "2026-11-04", "--releases", str(listing)]
        with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
            catalogue.main()
        with zipfile.ZipFile(self.dist / f"paperpaw-{VERSION}-catalogue.zip", "w") as z:
            for entry in sorted((self.plugins / "catalogue").glob("*.toml")):
                z.write(entry, f"catalogue/{entry.name}")
        return tag, listing

    def test_a_first_run_adds_every_plugin_file_to_the_store_release(self):
        tag, listing = self.run_catalogue([release("v0.6.0-beta.2", "2026-10-25", [("paperpaw-0.6.0-beta.2.apk", 1)])])
        self.assertEqual(tag, "plugins-2026")
        made = store_release.plan(store_release.load(listing), tag, store_release.files_of(self.plugins), repo=REPO)
        names = [f["name"] for f in made["files"]]
        self.assertTrue(made["create"])
        self.assertEqual(len(names), 3 * len(self.ids) + len(self.ids))
        self.assertTrue(all(n.endswith((".ppn", ".zip")) for n in names))
        planned = {f["name"]: (tag, f["sha256"]) for f in made["files"]}
        wrong = release_assets.check(self.dist, VERSION, REPO, planned, {}, checksum_of(listing))
        self.assertEqual(wrong, [])
        # The firmware release lists none of them.
        self.assertFalse(set(names) & set(release_assets.names(VERSION)))

    def test_a_plugin_published_before_keeps_its_address_and_is_not_uploaded_again(self):
        first = self.ids[0]
        version = catalogue.plugin_version(first)
        ppn = self.plugins / f"{first}-{version}-x4.ppn"
        old = [(ppn.name, ppn.stat().st_size, hashlib.sha256(ppn.read_bytes()).hexdigest())]
        tag, listing = self.run_catalogue([
            release("plugins-2026", "2026-11-01", old),
            release("v0.6.0-beta.2", "2026-10-25", [("paperpaw-0.6.0-beta.2.apk", 1)]),
        ])
        made = store_release.plan(store_release.load(listing), tag, store_release.files_of(self.plugins), repo=REPO)
        names = [f["name"] for f in made["files"]]
        self.assertNotIn(ppn.name, names)
        self.assertFalse(made["create"])
        planned = {f["name"]: (tag, f["sha256"]) for f in made["files"]}
        found = store_release.published.assets(json.dumps([store_release.load(listing)]))
        self.assertEqual(release_assets.check(self.dist, VERSION, REPO, planned, found, checksum_of(listing)), [])
        entry = (self.plugins / "catalogue" / f"{first}.toml").read_text(encoding="utf-8")
        self.assertIn(f"https://github.com/{REPO}/releases/download/plugins-2026/{ppn.name}", entry)

    def test_a_catalogue_address_without_a_planned_file_stops_the_release(self):
        tag, listing = self.run_catalogue([])
        made = store_release.plan(store_release.load(listing), tag, store_release.files_of(self.plugins), repo=REPO)
        planned = {f["name"]: (tag, f["sha256"]) for f in made["files"]}
        dropped = next(iter(planned))
        del planned[dropped]
        wrong = release_assets.check(self.dist, VERSION, REPO, planned, {}, checksum_of(listing))
        self.assertTrue(any(dropped in line for line in wrong), wrong)

    def test_a_published_name_with_other_bytes_stops_the_release_before_anything_is_made(self):
        first = self.ids[0]
        ppn = self.plugins / f"{first}-{catalogue.plugin_version(first)}-x4.ppn"
        # Published under this name with the same size and other bytes: a
        # fingerprint change without a new version of the plugin.
        old = [(ppn.name, ppn.stat().st_size, hashlib.sha256(b"other bytes, same name").hexdigest())]
        with contextlib.redirect_stderr(io.StringIO()) as said, self.assertRaises(SystemExit):
            self.run_catalogue([release("plugins-2026", "2026-11-01", old)])
        self.assertIn("raise the plugin's version", said.getvalue())
        listing = self.root / "releases.json"
        with self.assertRaises(store_release.Stop) as stopped:
            store_release.plan(store_release.load(listing), "plugins-2026", store_release.files_of(self.plugins), repo=REPO)
        self.assertIn("raise the plugin's version", str(stopped.exception))

    def test_a_run_started_again_after_it_broke_off_adds_only_what_is_missing(self):
        first = self.ids[0]
        ppn = self.plugins / f"{first}-{catalogue.plugin_version(first)}-x4.ppn"
        old = [(ppn.name, ppn.stat().st_size, hashlib.sha256(ppn.read_bytes()).hexdigest())]
        tag, listing = self.run_catalogue([release("plugins-2026", "2026-11-01", old)])
        made = store_release.plan(store_release.load(listing), tag, store_release.files_of(self.plugins), repo=REPO)
        self.assertNotIn(ppn.name, [f["name"] for f in made["files"]])
        self.assertIn(f"{ppn.name}: published in plugins-2026, the same bytes", made["published"])

    def test_a_full_store_release_gives_way_to_the_next(self):
        filler = [(f"old-{i}.ppn", 1) for i in range(790)]
        tag, listing = self.run_catalogue([release("plugins-2026", "2026-11-01", filler)])
        self.assertEqual(tag, "plugins-2026-2")
        made = store_release.plan(store_release.load(listing), tag, store_release.files_of(self.plugins), repo=REPO)
        self.assertTrue(made["create"])
        entry = (self.plugins / "catalogue" / f"{self.ids[0]}.toml").read_text(encoding="utf-8")
        self.assertIn("/releases/download/plugins-2026-2/", entry)

    def test_the_dry_run_prints_the_commands_and_changes_nothing(self):
        tag, listing = self.run_catalogue([])
        out = self.root / "plan.json"
        with contextlib.redirect_stdout(io.StringIO()):
            store_release.main(["plan", "--releases", str(listing), "--tag", tag, "--dist", str(self.plugins),
                                "--repo", REPO, "--out", str(out)])
        printed = io.StringIO()
        with contextlib.redirect_stdout(printed), unittest.mock.patch.object(store_release, "run") as run:
            store_release.main(["upload", "--plan", str(out), "--dry-run"])
        run.assert_not_called()
        lines = printed.getvalue().splitlines()
        self.assertTrue(lines[0].startswith("DRY RUN: gh release create plugins-2026"))
        self.assertTrue(all(line.startswith("DRY RUN: gh release") for line in lines))


if __name__ == "__main__":
    unittest.main()
