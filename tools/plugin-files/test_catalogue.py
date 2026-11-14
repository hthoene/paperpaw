"""Tests of catalogue.py, without a build: plugin files made up here.

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

# data.toml's version of the plugin these tests follow (a bump must not break them).
BI = catalogue.data.plugins()["literata-bold-italic"]["version"]

KEY = bytes(range(32))


def signed(fingerprint):
    """A plugin file as catalogue.py reads it: the header with its
    fingerprint, and a signature's trailer with the author's key."""
    header = b"PPNP" + struct.pack("<HH", 4, 5) + struct.pack("<Q", fingerprint)
    header += bytes(80 - len(header))
    trailer = b"PPNS" + bytes(8) + KEY
    return header + trailer + bytes(catalogue.SIGNATURE_LEN - len(trailer))


def signed5(target=5, major=7, minor=0, body=b""):
    """A format-5 file as catalogue.py reads it (abi/ppn5.md): a 96-byte header
    with the target, `abi_major` and `needs_minor`, and no fingerprint."""
    header = b"PPNP" + struct.pack("<HHHH", 5, target, major, minor)
    header += bytes(96 - len(header))
    trailer = b"PPNS" + bytes(8) + KEY
    return header + body + trailer + bytes(catalogue.SIGNATURE_LEN - len(trailer))


def built(dist):
    """A file for the X4 of each plugin of the store, named by its version."""
    for plugin, _ in catalogue.plugins():
        version = catalogue.plugin_version(plugin)
        dist.joinpath(f"{plugin}-{version}-x4.ppn").write_bytes(signed(0x85073A61323717FF))


class HowItWorks(unittest.TestCase):
    def test_every_plugin_of_the_store_has_one_in_english(self):
        for plugin, _ in catalogue.plugins():
            files = dict(catalogue.how_files(plugin))
            self.assertIn("how/en.txt", files, plugin)
            self.assertTrue(all("\r" not in text for text in files.values()), plugin)

    def test_only_languages_are_taken_with_lf(self):
        with tempfile.TemporaryDirectory() as root:
            how = pathlib.Path(root, "demo", "how")
            how.mkdir(parents=True)
            how.joinpath("en.txt").write_bytes(b"First.\r\n\r\nSecond.\r\n")
            how.joinpath("de.txt").write_text("Erstens.\n", encoding="utf-8")
            how.joinpath("README.txt").write_text("Not a language.\n", encoding="utf-8")
            how.joinpath("en.md").write_text("Not a text file.\n", encoding="utf-8")
            files = catalogue.how_files("demo", pathlib.Path(root))
        self.assertEqual(files, [("how/de.txt", "Erstens.\n"), ("how/en.txt", "First.\n\nSecond.\n")])

    def test_the_files_a_plugin_reads_from_the_card_go_in_its_folder(self):
        with tempfile.TemporaryDirectory() as root:
            demo = pathlib.Path(root, "demo")
            (demo / "pictures").mkdir(parents=True)
            (demo / "data").mkdir()
            demo.joinpath("Cargo.toml").write_text(
                '[package]\nname = "demo"\n[package.metadata.paperpaw]\ncard-files = ["pictures/*.png", "data/words.txt"]\n',
                encoding="utf-8",
            )
            demo.joinpath("data/embedded-only.ini").write_text("[a]\n", encoding="utf-8")
            demo.joinpath("pictures/owl.png").write_bytes(b"\x89PNG\r\n")
            demo.joinpath("data/words.txt").write_bytes(b"one\r\ntwo\r\n")
            files = catalogue.card_files("demo", pathlib.Path(root))
            self.assertEqual(files, [("pictures/owl.png", b"\x89PNG\r\n"), ("data/words.txt", b"one\ntwo\n")])
            # A kind a package may not hold stops the build: the plugin
            # library would refuse the package; so does a pattern of no file.
            demo.joinpath("data/page.html").write_text("<p>", encoding="utf-8")
            for pattern in ["data/page.html", "data/none-*.txt"]:
                demo.joinpath("Cargo.toml").write_text(
                    f'[package]\nname = "demo"\n[package.metadata.paperpaw]\ncard-files = ["{pattern}"]\n',
                    encoding="utf-8",
                )
                with self.assertRaises(SystemExit):
                    catalogue.card_files("demo", pathlib.Path(root))

    def test_the_plugins_that_read_the_card_have_their_files_in_their_folders(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            built(dist)
            argv = ["catalogue.py", str(dist), "0.4.0", "https://example.invalid/r", "--date", "2026-10-23"]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            for plugin, files in [
                ("vocabulary", {"data/phone.txt"}),
            ]:
                with zipfile.ZipFile(dist / f"{plugin}-{catalogue.plugin_version(plugin)}.zip") as z:
                    self.assertTrue(files <= set(z.namelist()), (plugin, z.namelist()))
                    self.assertLessEqual(len(z.namelist()), 63)

    def test_the_store_s_folders_carry_it(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            built(dist)
            argv = ["catalogue.py", str(dist), "0.4.0", "https://example.invalid/r", "--date", "2026-10-23"]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            folder = f"vocabulary-{catalogue.plugin_version('vocabulary')}.zip"
            first = dist.joinpath(folder).read_bytes()
            with zipfile.ZipFile(dist / folder) as z:
                names = z.namelist()
                english = z.read("how/en.txt").decode("utf-8")
            self.assertEqual(names[:1] + names[-2:], ["plugin.ini", "how/de.txt", "how/en.txt"])
            self.assertEqual(english, catalogue.ROOT.joinpath("plugins/vocabulary/how/en.txt").read_text(encoding="utf-8"))
            # The same bytes again: the release's checksums hold.
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            self.assertEqual(dist.joinpath(folder).read_bytes(), first)
            sums = dist.joinpath("SHA256SUMS").read_text(encoding="utf-8")
            self.assertIn(folder, sums)



class DataPlugins(unittest.TestCase):
    def folders(self, root):
        """The data plugins' folders as data.py builds them, made up."""
        for plugin, fields in catalogue.data.plugins().items():
            folder = root / plugin
            (folder / "fonts").mkdir(parents=True)
            folder.joinpath("plugin.ini").write_text(
                catalogue.data.plugin_ini(plugin, fields) + "formats = 3\n", encoding="utf-8")
            folder.joinpath("fonts", f"{plugin}.ppfp").write_bytes(b"PPFP" + bytes(range(200)))
            for path, text in catalogue.data.catalogs(plugin, fields):
                (folder / path).parent.mkdir(parents=True, exist_ok=True)
                (folder / path).write_text(text, encoding="utf-8")

    def test_a_release_s_data_plugins_come_with_their_folder_alone(self):
        with tempfile.TemporaryDirectory() as dist, tempfile.TemporaryDirectory() as made:
            dist, made = pathlib.Path(dist), pathlib.Path(made)
            built(dist)
            self.folders(made)
            argv = ["catalogue.py", str(dist), "0.5.0-beta.8", "https://example.invalid/r",
                    "--date", "2026-10-25", "--data", str(made)]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            entry = dist.joinpath("catalogue", "literata-bold-italic.toml").read_text(encoding="utf-8")
            self.assertIn('kind = "data"', entry)
            self.assertIn("off_by_default = true", entry)
            self.assertIn('name = "Literata Fett Kursiv"', entry)
            self.assertIn(f'url = "https://example.invalid/r/literata-bold-italic-{BI}.zip"', entry)
            for word in ["crate", "author_key", "builds"]:
                self.assertNotIn(word, entry)
            with zipfile.ZipFile(dist / f"literata-bold-italic-{BI}.zip") as z:
                self.assertEqual(z.namelist(), [
                    "plugin.ini", "fonts/literata-bold-italic.ppfp", "i18n/de.ini", "i18n/en.ini"])
                self.assertTrue(z.read("fonts/literata-bold-italic.ppfp").startswith(b"PPFP"))
            first = dist.joinpath(f"literata-bold-italic-{BI}.zip").read_bytes()
            sums = dist.joinpath("SHA256SUMS").read_text(encoding="utf-8")
            self.assertIn("literata-styles-1.1.0.zip", sums)
            # The same bytes again.
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            self.assertEqual(dist.joinpath(f"literata-bold-italic-{BI}.zip").read_bytes(), first)

    def test_one_data_plugin_alone_between_releases(self):
        with tempfile.TemporaryDirectory() as dist, tempfile.TemporaryDirectory() as made:
            dist, made = pathlib.Path(dist), pathlib.Path(made)
            self.folders(made)
            tag = f"literata-bold-italic-v{BI}"
            argv = ["catalogue.py", str(dist), "0.5.0-beta.8", f"https://example.invalid/{tag}",
                    "--only", "literata-bold-italic", "--tag", tag, "--data", str(made)]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            # Its entry alone, no plugin with code built or needed.
            self.assertEqual([p.name for p in dist.joinpath("catalogue").iterdir()], ["literata-bold-italic.toml"])
            entry = dist.joinpath("catalogue", "literata-bold-italic.toml").read_text(encoding="utf-8")
            self.assertIn(f'url = "https://example.invalid/{tag}/literata-bold-italic-{BI}.zip"', entry)
            sums = dist.joinpath("SHA256SUMS").read_text(encoding="utf-8")
            self.assertEqual(sums.count("\n"), 1)
            self.assertIn(f"literata-bold-italic-{BI}.zip", sums)

    def test_data_py_builds_one_alone(self):
        listed = catalogue.data.plugins()
        built = []

        def run(command, **_):
            # A builder as literata-packs.py is: every plugin it has.
            built.append(command[1])
            self.folders(pathlib.Path(command[2]))

        with tempfile.TemporaryDirectory() as out:
            argv = ["data.py", out, "--only", "literata-bold-italic"]
            with unittest.mock.patch.object(sys, "argv", argv), \
                    unittest.mock.patch.object(catalogue.data.subprocess, "run", run), \
                    contextlib.redirect_stdout(io.StringIO()) as said:
                catalogue.data.main()
        self.assertEqual(built, [str(catalogue.data.ROOT / listed["literata-bold-italic"]["build"])])
        self.assertIn("1 data plugins", said.getvalue())
        with unittest.mock.patch.object(sys, "argv", ["data.py", "x", "--only", "nothing"]), \
                self.assertRaises(SystemExit):
            catalogue.data.main()

    def test_a_folder_not_at_its_version_is_refused(self):
        with tempfile.TemporaryDirectory() as dist, tempfile.TemporaryDirectory() as made:
            dist, made = pathlib.Path(dist), pathlib.Path(made)
            built(dist)
            self.folders(made)
            ini = made / "literata-bold-italic" / "plugin.ini"
            ini.write_text(ini.read_text(encoding="utf-8").replace(f"version = {BI}", "version = 0.9.0"))
            argv = ["catalogue.py", str(dist), "0.5.0-beta.8", "https://example.invalid/r", "--data", str(made)]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()), \
                    contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                catalogue.main()


class Format5(unittest.TestCase):
    """Interface 7's files carry `7.<needs_minor>` where interface 6's carry the
    fingerprint (Kernel's ruling on #228, todo 169)."""

    def run_catalogue(self, dist, *extra):
        argv = ["catalogue.py", str(dist), "0.6.0", "https://example.invalid/r", "--date", "2026-11-08",
                "--only", "vocabulary", *extra]
        with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
            catalogue.main()

    def test_the_abi_of_a_format_5_file_is_its_major_and_minor_in_decimal(self):
        with tempfile.NamedTemporaryFile() as f:
            path = pathlib.Path(f.name)
            for minor in (0, 3, 12, 65535):
                path.write_bytes(signed5(minor=minor))
                abi, key, file_format, target = catalogue.ppn(path)
                self.assertEqual((abi, key, file_format, target), (f"7.{minor}", KEY.hex(), 5, 5))
            # A format-4 file is still its fingerprint.
            path.write_bytes(signed(0x85073A61323717FF))
            self.assertEqual(catalogue.ppn(path)[0], "85073a61323717ff")
            self.assertEqual(catalogue.ppn(path)[2], 4)

    def test_other_formats_and_majors_are_refused(self):
        with tempfile.NamedTemporaryFile() as f:
            path = pathlib.Path(f.name)
            for bad in (signed5(major=6), signed5(major=8), signed(1)[:4] + struct.pack("<H", 3) + signed(1)[6:],
                        signed(1)[:4] + struct.pack("<H", 6) + signed(1)[6:]):
                path.write_bytes(bad)
                with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                    catalogue.ppn(path)
            # Unsigned.
            path.write_bytes(signed5()[: -catalogue.SIGNATURE_LEN])
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                catalogue.ppn(path)

    def test_a_file_cut_short_is_refused_cleanly_not_with_a_struct_error(self):
        # 12 to 15 bytes of a format-4 file have the format but not the
        # fingerprint (bytes 8..16); every length below a header is refused.
        with tempfile.NamedTemporaryFile() as f:
            path = pathlib.Path(f.name)
            for full in (signed(0x85073A61323717FF), signed5()):
                for length in range(0, 96):
                    path.write_bytes(full[:length])
                    err = io.StringIO()
                    with contextlib.redirect_stderr(err), self.assertRaises(SystemExit):
                        catalogue.ppn(path)
                    self.assertIn("catalogue.py:", err.getvalue(), length)

    def test_the_entry_of_a_format_5_build_says_its_abi_and_where_it_is_built(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            own = catalogue.plugin_version("vocabulary")
            dist.joinpath(f"vocabulary-{own}-x4.ppn").write_bytes(signed5(5, minor=2))
            dist.joinpath(f"vocabulary-{own}-x4-pro.ppn").write_bytes(signed5(9, minor=2))
            self.run_catalogue(dist)
            entry = dist.joinpath("catalogue", "vocabulary.toml").read_text(encoding="utf-8")
            self.assertIn('abi = "7.2"', entry)
            self.assertNotIn("abi = \"00", entry)
            self.assertIn('path = "tools/plugin-files7"', entry)

    def test_a_format_4_entry_keeps_its_source_and_fingerprint(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            own = catalogue.plugin_version("vocabulary")
            dist.joinpath(f"vocabulary-{own}-x4.ppn").write_bytes(signed(0x85073A61323717FF))
            self.run_catalogue(dist)
            entry = dist.joinpath("catalogue", "vocabulary.toml").read_text(encoding="utf-8")
            self.assertIn('abi = "85073a61323717ff"', entry)
            self.assertIn('path = "tools/plugin-files"', entry)

    def test_a_file_for_another_chip_than_its_device_is_refused(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            own = catalogue.plugin_version("vocabulary")
            dist.joinpath(f"vocabulary-{own}-x4-classic.ppn").write_bytes(signed5(5))
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                self.run_catalogue(dist)

    def test_one_entry_never_mixes_formats(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            own = catalogue.plugin_version("vocabulary")
            dist.joinpath(f"vocabulary-{own}-x4.ppn").write_bytes(signed5(5))
            dist.joinpath(f"vocabulary-{own}-x4-pro.ppn").write_bytes(signed(0x85073A61323717FF))
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                self.run_catalogue(dist)


class Versions(unittest.TestCase):
    def test_a_release_s_plugins_carry_their_own_versions(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            built(dist)
            argv = ["catalogue.py", str(dist), "0.5.0-beta.2", "https://example.invalid/r", "--date", "2026-10-24"]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            own = catalogue.plugin_version("vocabulary")
            entry = dist.joinpath("catalogue", "vocabulary.toml").read_text(encoding="utf-8")
            # Its crate's version, not PaperPaw's; the SDK and the source
            # the release's.
            self.assertIn(f'version = "{own}"', entry)
            self.assertNotIn('version = "0.5.0-beta.2"', entry)
            self.assertIn('sdk = "0.5"', entry)
            self.assertIn('revision = "v0.5.0-beta.2"', entry)
            with zipfile.ZipFile(dist / f"vocabulary-{own}.zip") as z:
                self.assertIn(f"version = {own}" + chr(10), z.read("plugin.ini").decode("utf-8"))

    def test_one_plugin_alone_between_releases(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            own = catalogue.plugin_version("vocabulary")
            dist.joinpath(f"vocabulary-{own}-x4.ppn").write_bytes(signed(0x85073A61323717FF))
            argv = ["catalogue.py", str(dist), "0.5.0-beta.2", "https://example.invalid/r",
                    "--only", "vocabulary", "--tag", f"vocabulary-v{own}"]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            # Its entry alone, so the store keeps the others' versions.
            self.assertEqual([p.name for p in dist.joinpath("catalogue").iterdir()], ["vocabulary.toml"])
            entry = dist.joinpath("catalogue", "vocabulary.toml").read_text(encoding="utf-8")
            self.assertIn(f'version = "{own}"', entry)
            self.assertIn('sdk = "0.5"', entry)
            self.assertIn(f'revision = "vocabulary-v{own}"', entry)
            sums = dist.joinpath("SHA256SUMS").read_text(encoding="utf-8")
            self.assertNotIn("aids", sums)

    def test_a_file_published_before_keeps_the_address_of_its_release(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            own = catalogue.plugin_version("vocabulary")
            ppn = f"vocabulary-{own}-x4.ppn"
            dist.joinpath(ppn).write_bytes(signed(0x85073A61323717FF))
            old = "https://github.com/o/r/releases/download/plugins-2026/" + ppn
            listing = dist / "releases.json"
            listing.write_text(json.dumps([[{
                "tag_name": "plugins-2026", "created_at": "2026-11-01", "draft": False,
                "assets": [{"name": ppn, "browser_download_url": old, "size": 1, "state": "uploaded",
                            "digest": "sha256:" + hashlib.sha256(dist.joinpath(ppn).read_bytes()).hexdigest()}]}]]))
            argv = ["catalogue.py", str(dist), "0.6.0-beta.3", "https://github.com/o/r/releases/download/plugins-2026-2",
                    "--only", "vocabulary", "--releases", str(listing)]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()):
                catalogue.main()
            entry = dist.joinpath("catalogue", "vocabulary.toml").read_text(encoding="utf-8")
            # The file published before: where it is. The folder, new: the release this run adds to.
            self.assertIn(f'url = "{old}"', entry)
            self.assertIn(f'url = "https://github.com/o/r/releases/download/plugins-2026-2/vocabulary-{own}.zip"', entry)

    def test_a_file_published_before_with_other_bytes_is_refused(self):
        with tempfile.TemporaryDirectory() as dist:
            dist = pathlib.Path(dist)
            own = catalogue.plugin_version("vocabulary")
            ppn = f"vocabulary-{own}-x4.ppn"
            dist.joinpath(ppn).write_bytes(signed(0x85073A61323717FF))
            listing = dist / "releases.json"
            listing.write_text(json.dumps([[{
                "tag_name": "plugins-2026", "created_at": "2026-11-01", "draft": False,
                "assets": [{"name": ppn, "size": dist.joinpath(ppn).stat().st_size, "state": "uploaded",
                            "browser_download_url": "https://github.com/o/r/releases/download/plugins-2026/" + ppn,
                            "digest": "sha256:" + hashlib.sha256(b"other bytes").hexdigest()}]}]]))
            argv = ["catalogue.py", str(dist), "0.6.0-beta.3", "https://github.com/o/r/releases/download/plugins-2026",
                    "--only", "vocabulary", "--releases", str(listing)]
            said = io.StringIO()
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stdout(io.StringIO()), \
                    contextlib.redirect_stderr(said), self.assertRaises(SystemExit):
                catalogue.main()
            self.assertIn("raise the plugin's version", said.getvalue())

    def test_a_plugin_without_a_version_of_its_own_is_refused(self):
        with tempfile.TemporaryDirectory() as root:
            crate = pathlib.Path(root, "demo")
            crate.mkdir()
            crate.joinpath("Cargo.toml").write_text('[package]\nname = "demo"\nversion.workspace = true\n')
            with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                catalogue.plugin_version("demo", pathlib.Path(root))
            crate.joinpath("Cargo.toml").write_text('[package]\nname = "demo"\nversion = "1.2.0"\n')
            self.assertEqual(catalogue.plugin_version("demo", pathlib.Path(root)), "1.2.0")

    def test_only_a_plugin_of_the_store(self):
        with tempfile.TemporaryDirectory() as dist:
            argv = ["catalogue.py", dist, "0.5.0-beta.3", "https://example.invalid/r", "--only", "nonesuch"]
            with unittest.mock.patch.object(sys, "argv", argv), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit):
                    catalogue.main()


if __name__ == "__main__":
    unittest.main()
