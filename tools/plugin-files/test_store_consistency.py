"""Tests of store_consistency.py: what store-plugin.yml checks of a batch of plugins against the released interface.

    python3 -m unittest discover -s tools/plugin-files
"""

import contextlib
import io
import json
import pathlib
import struct
import subprocess
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import store_consistency as sc  # noqa: E402

NATIVE = "pub const INTERFACE: u16 = 7;\npub const DRAFT: u16 = 36;\n"
ABI = "pub const MAJOR: u16 = 7;\npub const MINOR: u16 = 1;\n"


def record(minor, major=7):
    return b"PPAM" + struct.pack("<HHIHH", 2, 32, 88, major, minor) + bytes(16)


def entry(version, abis):
    builds = "".join(f'\n[[versions.builds]]\ndevice = "{d}"\nabi = "{a}"\n' for d, a in abis.items())
    return f'[[versions]]\nversion = "{version}"\n{builds}'


def run(*argv):
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        code = sc.main(list(argv))
    return code, out.getvalue().strip(), err.getvalue()


class Repo(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = pathlib.Path(self.tmp.name)
        self.git("init", "-q")
        self.write("crates/paperpaw-sdk/src/native.rs", NATIVE)
        self.write("crates/paperpaw-native/src/host_glue/paperpaw_abi.rs", ABI)
        self.write("Cargo.toml", '[workspace.package]\nversion = "0.6.0-beta.4"\n')
        self.write("tools/plugin-files/plugins.txt", "aids reading\nstats reading\n")
        for plugin in ("aids", "stats"):
            self.plugin(plugin, "1.0.0")
        self.commit("base")
        self.git("tag", "v0.6.0-beta.4")

    def git(self, *args):
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args], cwd=self.root, check=True, capture_output=True)

    def write(self, path, text):
        full = self.root / path
        full.parent.mkdir(parents=True, exist_ok=True)
        full.write_bytes(text if isinstance(text, bytes) else text.encode())

    def plugin(self, plugin, version, minor=1):
        self.write(f"plugins/{plugin}/Cargo.toml", f'[package]\nname = "p-{plugin}"\nversion = "{version}"\n')
        self.write(f"plugins/{plugin}/native/metadata.bin", record(minor))
        self.write(f"plugins/{plugin}/native/metadata.s3.bin", record(minor))

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def releases(self, *names):
        path = self.root / "releases.json"
        assets = [{"name": n, "state": "uploaded", "size": 1, "digest": None, "browser_download_url": "u"} for n in names]
        path.write_text(json.dumps([[{"tag_name": "plugins-2026", "draft": False, "created_at": "2026-01-01", "assets": assets}]]))
        return str(path)

    def check(self, *ids, releases=None):
        extra = ["--releases", releases] if releases else []
        return run("check", *ids, "--base", "v0.6.0-beta.4", "--source", str(self.root), *extra)


class Check(Repo):
    def test_a_plugin_the_release_accepts_passes_and_names_its_abi(self):
        code, out, err = self.check("aids", "stats")
        self.assertEqual((code, out), (0, "7.1"), err)

    def test_a_plugin_that_needs_a_newer_minor_than_the_release_is_refused(self):
        self.plugin("aids", "1.0.1", minor=2)
        code, _, err = self.check("aids")
        self.assertEqual(code, 1)
        self.assertIn("needs ABI 7.2", err)
        self.assertIn("released firmware has 7.1", err)

    def test_the_s3_record_counts_too(self):
        self.write("plugins/aids/native/metadata.s3.bin", record(2))
        self.assertIn("metadata.s3.bin", self.check("aids")[2])

    def test_another_abi_major_or_interface_needs_a_firmware(self):
        self.write("plugins/aids/native/metadata.bin", record(1, major=8))
        self.assertIn("ABI major 8", self.check("aids")[2])
        self.write("crates/paperpaw-sdk/src/native.rs", NATIVE.replace("= 7", "= 8"))
        self.assertIn("plugin interface 8", self.check("stats")[2])

    def test_a_newer_draft_is_only_a_note(self):
        self.write("crates/paperpaw-sdk/src/native.rs", NATIVE.replace("36", "37"))
        code, _, err = self.check("aids")
        self.assertEqual(code, 0)
        self.assertIn("draft 37", err)

    def test_a_base_without_interface_7_is_refused(self):
        self.write("crates/paperpaw-native/src/host_glue/paperpaw_abi.rs", "")
        self.commit("old")
        self.git("tag", "old")
        code, _, err = run("check", "aids", "--base", "old", "--source", str(self.root))
        self.assertEqual(code, 1)
        self.assertIn("no interface-7 ABI", err)

    def test_a_changed_plugin_needs_a_newer_version_than_the_published_one(self):
        self.git("tag", "aids-v1.0.0")
        published = self.releases("aids-1.0.0-x4.ppn", "aids-1.0.0-x4-pro.ppn")
        self.assertEqual(self.check("aids", releases=published)[0], 0)  # unchanged: nothing new
        self.write("plugins/aids/src/lib.rs", "fn changed() {}")
        self.commit("change")
        code, _, err = self.check("aids", releases=published)
        self.assertEqual(code, 1)
        self.assertIn("still 1.0.0", err)
        self.plugin("aids", "1.0.1")
        self.commit("bump")
        self.assertEqual(self.check("aids", releases=published)[0], 0)

    def test_a_version_older_than_the_published_one_is_refused(self):
        published = self.releases("aids-1.0.5-x4.ppn")
        self.assertIn("cannot go back", self.check("aids", releases=published)[2])

    def test_a_build_published_for_another_interface_needs_a_new_version(self):
        self.write("crates/paperpaw-sdk/src/native.rs", NATIVE.replace("= 7", "= 6"))
        self.commit("interface 6")
        self.git("tag", "v0.6.0-beta.3")
        self.write("crates/paperpaw-sdk/src/native.rs", NATIVE)
        self.commit("interface 7")
        published = self.releases("aids-1.0.0-x4.ppn", "stats-1.0.0-x4.ppn")
        code, _, err = self.check("aids", releases=published)
        self.assertEqual(code, 1)
        self.assertIn("interface 6", err)

    def test_changed_selects_what_has_something_to_publish(self):
        self.git("tag", "aids-v1.0.0")
        self.git("tag", "stats-v1.0.0")
        published = self.releases("aids-1.0.0-x4.ppn", "stats-1.0.0-x4.ppn")
        args = ["--base", "v0.6.0-beta.4", "--source", str(self.root), "--releases", published]
        self.assertEqual(run("select", "changed", *args)[1], "")
        self.plugin("stats", "1.0.1")
        self.commit("bump")
        self.assertEqual(run("select", "changed", *args)[1], "stats")
        self.assertEqual(run("select", "all", *args)[1], "aids stats")
        self.assertEqual(run("check", "changed", *args)[0], 0)


def header(fmt=5, target=5, major=7, minor=1):
    return b"PPNP" + struct.pack("<HHHH", fmt, target, major, minor) + bytes(84)


class Files(unittest.TestCase):
    def folder(self, **files):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        for name, data in files.items():
            pathlib.Path(tmp.name, name).write_bytes(data)
        return pathlib.Path(tmp.name)

    def test_the_header_is_held_to_the_released_firmware(self):
        folder = self.folder(**{"aids-1.0.0-x4.ppn": header(), "stats-1.0.0-x4.ppn": header(minor=2)})
        problems = sc.file_problems(folder, "x4", (7, 1), [("aids", "1.0.0"), ("stats", "1.0.0"), ("keys", "1.0.0")])
        self.assertEqual(len(problems), 2)
        self.assertIn("ABI 7.2", problems[0])
        self.assertIn("not built", problems[1])

    def test_chip_and_format(self):
        folder = self.folder(**{"a-1.0.0-x4-pro.ppn": header(target=5), "b-1.0.0-x4-pro.ppn": header(fmt=4, target=9),
                                "c-1.0.0-x4-pro.ppn": header(target=9)})
        problems = sc.file_problems(folder, "x4-pro", (7, 1), [("a", "1.0.0"), ("b", "1.0.0"), ("c", "1.0.0")])
        self.assertEqual(len(problems), 2)
        self.assertIn("target 5", problems[0])
        self.assertIn("format 4", problems[1])


class Entries(unittest.TestCase):
    ok = {"x4": "7.1", "x4-pro": "7.1", "x4-classic": "7.1"}

    def test_the_entry_must_say_the_version_and_an_abi_the_release_accepts(self):
        plugins = [("aids", "1.1.0")]
        self.assertEqual(sc.entry_problems({"aids": entry("1.1.0", self.ok)}, (7, 1), plugins, []), [])
        bad = sc.entry_problems({"aids": entry("1.1.0", {**self.ok, "x4": "7.2", "x4-classic": "0123456789abcdef"})}, (7, 1), plugins, [])
        self.assertEqual(len(bad), 2)
        self.assertTrue(any("7.2" in p for p in bad) and any("fingerprint" in p for p in bad))
        self.assertIn("no version 1.1.0", sc.entry_problems({"aids": entry("1.0.9", self.ok)}, (7, 1), plugins, [])[0])
        self.assertIn("older than 1.2.0", sc.entry_problems({"aids": entry("1.1.0", self.ok)}, (7, 1), plugins, ["aids-1.2.0-x4.ppn"])[0])
        self.assertIn("no build for x4-classic", sc.entry_problems({"aids": entry("1.1.0", {"x4": "7.1", "x4-pro": "7.1"})}, (7, 1), plugins, [])[0])

    def test_entries_are_read_from_a_zip(self):
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        path = pathlib.Path(tmp.name, "c.zip")
        with zipfile.ZipFile(path, "w") as archive:
            archive.writestr("catalogue/aids.toml", entry("1.1.0", self.ok))
        self.assertEqual(sc.main(["entries", "--abi", "7.1", "--catalogue", str(path), "aids=1.1.0"]), 0)


if __name__ == "__main__":
    unittest.main()
