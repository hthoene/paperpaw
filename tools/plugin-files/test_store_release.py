"""Tests of store_release.py: GitHub's list and the `gh` commands made up here.

    python3 -m unittest discover -s tools/plugin-files
"""

import io
import contextlib
import json
import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import store_release  # noqa: E402


def release(tag, count=0, names=(), created="2026-11-01", sizes=None):
    assets = [{"name": n, "size": (sizes or {}).get(n, 3), "browser_download_url": f"https://r/{tag}/{n}"}
              for n in names]
    assets += [{"name": f"filler-{i}.ppn", "size": 1, "browser_download_url": f"https://r/{tag}/f{i}"}
               for i in range(count)]
    return {"tag_name": tag, "created_at": created, "draft": False, "assets": assets}


class Choose(unittest.TestCase):
    def test_the_first_store_release_of_the_year_is_made(self):
        self.assertEqual(store_release.choose([release("v0.6.0-beta.2", 251)], 2026, 100), ("plugins-2026", False))

    def test_the_newest_one_takes_files_while_it_has_room(self):
        releases = [release("plugins-2026", 700)]
        self.assertEqual(store_release.choose(releases, 2026, 100), ("plugins-2026", True))

    def test_a_full_one_gives_way_to_the_next_number(self):
        releases = [release("plugins-2026", 790)]
        self.assertEqual(store_release.choose(releases, 2026, 20), ("plugins-2026-2", False))
        releases.append(release("plugins-2026-2", 100, created="2026-12-01"))
        self.assertEqual(store_release.choose(releases, 2026, 20), ("plugins-2026-2", True))
        releases[1] = release("plugins-2026-2", 790, created="2026-12-01")
        self.assertEqual(store_release.choose(releases, 2026, 20), ("plugins-2026-3", False))

    def test_another_year_starts_its_own(self):
        self.assertEqual(store_release.choose([release("plugins-2026", 10)], 2027, 5), ("plugins-2027", False))

    def test_more_than_a_release_holds_is_refused(self):
        with self.assertRaises(store_release.Stop):
            store_release.choose([], 2026, 801)

    def test_a_store_tag_never_looks_like_a_firmware_tag(self):
        for tag in ("plugins-2026", "plugins-2026-2", "plugins-2099-17"):
            self.assertTrue(store_release.STORE_TAG.match(tag))
            self.assertFalse(tag.startswith("v") and tag[1:2].isdigit())
        for tag in ("plugins-26", "plugins-2026-1", "plugins-2026-0", "v0.6.0", "news-v1.0.2"):
            self.assertIsNone(store_release.STORE_TAG.match(tag))


class Plan(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.dist = pathlib.Path(self.dir.name)

    def tearDown(self):
        self.dir.cleanup()

    def put(self, name, body=b"abc"):
        path = self.dist / name
        path.write_bytes(body)
        return path

    def test_only_files_not_published_before_are_planned(self):
        self.put("news-1.0.2-x4.ppn")
        self.put("news-1.0.3-x4.ppn")
        self.put("news-1.0.3.zip")
        self.put("SHA256SUMS")
        (self.dist / "catalogue").mkdir()
        releases = [release("plugins-2026", 5, ["news-1.0.2-x4.ppn"]), release("v0.6.0-beta.2", 0)]
        made = store_release.plan(releases, "plugins-2026", store_release.files_of(self.dist), repo="o/r")
        self.assertEqual([f["name"] for f in made["files"]], ["news-1.0.3-x4.ppn", "news-1.0.3.zip"])
        self.assertEqual(made["held"], 6)
        self.assertEqual(made["after"], 8)
        self.assertFalse(made["create"])
        self.assertEqual(made["published"], ["news-1.0.2-x4.ppn: published in plugins-2026"])

    def test_a_published_name_with_other_bytes_needs_a_newer_version(self):
        self.put("news-1.0.2-x4.ppn", b"other bytes than published")
        releases = [release("v0.6.0-beta.2", 0, ["news-1.0.2-x4.ppn"], sizes={"news-1.0.2-x4.ppn": 3})]
        with self.assertRaises(store_release.Stop) as stopped:
            store_release.plan(releases, "plugins-2026", store_release.files_of(self.dist))
        self.assertIn("newer version", str(stopped.exception))

    def test_a_release_that_is_not_there_is_made(self):
        self.put("a-1.0.0-x4.ppn")
        made = store_release.plan([], "plugins-2026", store_release.files_of(self.dist))
        self.assertTrue(made["create"])
        self.assertEqual(made["held"], 0)

    def test_the_tripwire(self):
        self.put("a-1.0.0-x4.ppn")
        with self.assertRaises(store_release.Stop):
            store_release.plan([release("plugins-2026", 900)], "plugins-2026", store_release.files_of(self.dist))

    def test_extras_are_planned_too_unless_published(self):
        extra = self.put("paperpaw-news-v1.0.3-catalogue.zip")
        made = store_release.plan([release("plugins-2026", 1)], "plugins-2026", [], [extra])
        self.assertEqual([f["name"] for f in made["files"]], ["paperpaw-news-v1.0.3-catalogue.zip"])


class Commands(unittest.TestCase):
    def plan(self, count, create):
        return {"repo": "o/r", "tag": "plugins-2026", "create": create, "held": 0, "after": count,
                "files": [{"name": f"f{i}.ppn", "path": f"dist/f{i}.ppn", "size": 3, "sha256": "x"}
                          for i in range(count)], "published": []}

    def test_a_pre_release_that_is_never_the_latest_and_files_never_replaced(self):
        cmds = store_release.commands(self.plan(45, True))
        self.assertEqual(cmds[0][:4], ["gh", "release", "create", "plugins-2026"])
        self.assertIn("--prerelease", cmds[0])
        self.assertIn("--latest=false", cmds[0])
        self.assertEqual(len(cmds), 1 + 3)
        for argv in cmds:
            self.assertNotIn("--clobber", argv)
        self.assertEqual(sum(len(c) - 6 for c in cmds[1:]), 45)

    def test_no_create_for_a_release_that_exists(self):
        self.assertEqual(store_release.commands(self.plan(2, False))[0][:3], ["gh", "release", "upload"])


class Upload(unittest.TestCase):
    def fake(self, fail_batches=0, present=None):
        calls, assets = [], dict(present or {})
        state = {"failed": 0}

        def runner(argv):
            calls.append(argv)
            if argv[:3] == ["gh", "release", "create"]:
                return 0, ""
            if argv[:2] == ["gh", "api"]:
                return 0, "".join(f"{n} {s}\n" for n, s in assets.items())
            if argv[:3] == ["gh", "release", "upload"]:
                if state["failed"] < fail_batches:
                    state["failed"] += 1
                    return 1, "network"
                for path in argv[argv.index("-R") + 2:]:
                    assets[pathlib.Path(path).name] = pathlib.Path(path).stat().st_size
                return 0, ""
            raise AssertionError(argv)

        return runner, calls, assets

    def plan(self, root, count):
        files = []
        for i in range(count):
            path = pathlib.Path(root, f"f{i}.ppn")
            path.write_bytes(b"abc")
            files.append({"name": path.name, "path": str(path), "size": 3, "sha256": "x"})
        return {"repo": "o/r", "tag": "plugins-2026", "create": True, "held": 0, "after": count,
                "files": files, "published": []}

    def test_all_files_go_up_and_the_listing_is_checked(self):
        with tempfile.TemporaryDirectory() as root:
            runner, calls, assets = self.fake()
            store_release.upload(self.plan(root, 25), runner, sleep=lambda s: None, say=lambda m: None)
            self.assertEqual(len(assets), 25)

    def test_a_failed_batch_goes_file_by_file_and_a_file_already_there_is_skipped(self):
        with tempfile.TemporaryDirectory() as root:
            runner, calls, assets = self.fake(fail_batches=1, present={"f0.ppn": 3})
            store_release.upload(self.plan(root, 3), runner, sleep=lambda s: None, say=lambda m: None)
            self.assertEqual(sorted(assets), ["f0.ppn", "f1.ppn", "f2.ppn"])
            singles = [c for c in calls if c[:3] == ["gh", "release", "upload"] and len(c) == 7]
            self.assertEqual([pathlib.Path(c[-1]).name for c in singles], ["f1.ppn", "f2.ppn"])

    def test_a_file_that_never_goes_up_stops_the_run(self):
        with tempfile.TemporaryDirectory() as root:
            runner, calls, assets = self.fake(fail_batches=99)
            with self.assertRaises(store_release.Stop):
                store_release.upload(self.plan(root, 2), runner, sleep=lambda s: None, say=lambda m: None)

    def test_a_listing_that_lacks_a_file_fails_the_check(self):
        with tempfile.TemporaryDirectory() as root:
            runner, calls, assets = self.fake()

            def lossy(argv):
                code, out = runner(argv)
                if argv[:2] == ["gh", "api"]:
                    out = "\n".join(out.splitlines()[:-1])
                return code, out

            with self.assertRaises(store_release.Stop):
                store_release.upload(self.plan(root, 3), lossy, sleep=lambda s: None, say=lambda m: None)


class Versions(unittest.TestCase):
    names = ["news-1.0.2-x4.ppn", "news-1.0.2.zip", "literata-bold-italic-2.0.0.zip", "news-1.0.10-x4.ppn"]

    def test_a_new_version_is_listed(self):
        releases = [release("plugins-2026", 0, self.names)]
        self.assertEqual(store_release.versions(releases, {"news": "1.0.11"}), ["news 1.0.11"])

    def test_a_version_published_is_refused(self):
        releases = [release("plugins-2026", 0, self.names)]
        with self.assertRaises(store_release.Stop):
            store_release.versions(releases, {"news": "1.0.10"})

    def test_an_older_version_than_a_published_one_is_refused(self):
        releases = [release("plugins-2026", 0, self.names)]
        with self.assertRaises(store_release.Stop):
            store_release.versions(releases, {"news": "1.0.9"})

    def test_ids_with_hyphens_and_a_plugin_not_published_yet(self):
        releases = [release("plugins-2026", 0, self.names)]
        self.assertEqual(store_release.versions(releases, {"literata-bold": "1.0.0", "literata-bold-italic": "2.0.1"}),
                         ["literata-bold 1.0.0", "literata-bold-italic 2.0.1"])

    def test_the_cli_prints_the_choice(self):
        with tempfile.TemporaryDirectory() as root:
            listing = pathlib.Path(root, "r.json")
            listing.write_text(json.dumps([[release("plugins-2026", 10)]]))
            dist = pathlib.Path(root, "dist")
            dist.mkdir()
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                store_release.main(["choose", "--releases", str(listing), "--year", "2026", "--dist", str(dist),
                                    "--incoming", "5"])
            self.assertEqual(out.getvalue().strip(), "plugins-2026")


if __name__ == "__main__":
    unittest.main()
