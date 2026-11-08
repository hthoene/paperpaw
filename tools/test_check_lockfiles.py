"""check-lockfiles.py: a stale lock and an orphan entry are found, a good lock passes."""

import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("check_lockfiles", HERE / "check-lockfiles.py")
check_lockfiles = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check_lockfiles)

LOCK = """version = 4

[[package]]
name = "app"
version = "1.0.0"
dependencies = [
 "leaf",
 "twin 1.0.0",
]

[[package]]
name = "leaf"
version = "0.2.0"

[[package]]
name = "twin"
version = "1.0.0"

[[package]]
name = "twin"
version = "2.0.0"
"""


class Reachability(unittest.TestCase):
    def test_walks_from_members_and_follows_versioned_references(self):
        lock = tomllib.loads(LOCK)
        self.assertEqual(
            check_lockfiles.unreachable(lock, {("app", "1.0.0")}), [("twin", "2.0.0")]
        )

    def test_a_member_with_no_entry_does_not_hide_the_rest(self):
        lock = tomllib.loads(LOCK)
        self.assertEqual(len(check_lockfiles.unreachable(lock, set())), 4)


@unittest.skipUnless(shutil.which("cargo"), "needs cargo")
class RealWorkspace(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.dir = Path(temporary.name).resolve()
        (self.dir / "leaf/src").mkdir(parents=True)
        (self.dir / "app/src").mkdir(parents=True)
        (self.dir / "leaf/Cargo.toml").write_text('[package]\nname = "leaf"\nversion = "0.2.0"\nedition = "2021"\n')
        (self.dir / "leaf/src/lib.rs").write_text("")
        (self.dir / "app/Cargo.toml").write_text(
            '[package]\nname = "app"\nversion = "1.0.0"\nedition = "2021"\n'
            '[dependencies]\nleaf = { path = "../leaf" }\n[workspace]\n'
        )
        (self.dir / "app/src/lib.rs").write_text("")
        self.env = dict(os.environ)
        self.lock = self.dir / "app/Cargo.lock"
        subprocess.run(
            ["cargo", "generate-lockfile", "--offline", "--manifest-path", str(self.dir / "app/Cargo.toml")],
            check=True, env=self.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        # `check` prints paths relative to the repository; use a lock outside it.
        check_lockfiles.ROOT = self.dir

    def test_a_good_lock_passes(self):
        self.assertEqual(check_lockfiles.check(self.lock, self.env), [])

    def test_a_stale_version_is_found(self):
        text = self.lock.read_text()
        self.assertIn('name = "leaf"\nversion = "0.2.0"', text)
        self.lock.write_text(text.replace('name = "leaf"\nversion = "0.2.0"', 'name = "leaf"\nversion = "0.1.0"'))
        found = check_lockfiles.check(self.lock, self.env)
        self.assertEqual(len(found), 1)
        self.assertIn("stale for its manifests", found[0])

    def test_an_entry_nothing_needs_is_found(self):
        with self.lock.open("a") as stream:
            stream.write('\n[[package]]\nname = "gone"\nversion = "3.0.0"\n')
        found = check_lockfiles.check(self.lock, self.env)
        self.assertEqual(len(found), 1)
        self.assertIn("gone 3.0.0", found[0])


if __name__ == "__main__":
    unittest.main()
