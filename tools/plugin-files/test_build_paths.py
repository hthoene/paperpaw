"""build.sh and plugin-files7/build.sh refuse a path with a space (the remap of
the paths cannot be made); PAPERPAW_ALLOW_UNREMAPPED=1 allows a trial build and
is never read for a signed (--key) or fingerprint-checked build.

A `cargo` that only notes it was called stands in for the real one: the checks
come before any build, so nothing is compiled here.

    python3 -m unittest discover -s tools/plugin-files
"""

import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).resolve().parents[2]
# Each script with a plugin it knows (plugins.txt; the native entries).
SCRIPTS = {
    "plugin-files": (ROOT / "tools/plugin-files/build.sh", "aids"),
    "plugin-files7": (ROOT / "tools/plugin-files7/build.sh", "battery"),
}


class SpacedPaths(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        tmp = pathlib.Path(self.tmp.name)
        self.bin = tmp / "bin"
        self.bin.mkdir()
        self.called = tmp / "cargo-called"
        fake = self.bin / "cargo"
        fake.write_text(f'#!/bin/sh\necho called >> "{self.called}"\nexit 99\n')
        fake.chmod(0o755)
        self.spaced = tmp / "a b"
        self.out = tmp / "out"

    def run_script(self, name, *args, override=False):
        env = {k: v for k, v in os.environ.items()
               # BASH_ENV: a profile it names may reset PATH, and then the real
               # cargo would build.
               if k not in ("RUSTFLAGS", "PAPERPAW_ALLOW_UNREMAPPED", "CARGO_ENCODED_RUSTFLAGS", "BASH_ENV")}
        env["PATH"] = f"{self.bin}:{env['PATH']}"
        env["CARGO_TARGET_DIR"] = str(self.spaced)
        if override:
            env["PAPERPAW_ALLOW_UNREMAPPED"] = "1"
        script, plugin = SCRIPTS[name]
        proc = subprocess.run(
            ["bash", str(script), "x4", str(self.out), "--only", plugin, *args],
            env=env, capture_output=True, text=True, timeout=120)
        return proc

    def test_a_spaced_path_is_refused_before_any_build(self):
        for name in SCRIPTS:
            proc = self.run_script(name)
            self.assertNotEqual(proc.returncode, 0, name)
            self.assertIn("has a space or tab", proc.stderr, (name, proc.stderr))
            self.assertIn("error:", proc.stderr, name)
            self.assertFalse(self.called.exists(), f"{name} called cargo")

    def test_the_override_allows_a_trial_build(self):
        for name in SCRIPTS:
            proc = self.run_script(name, override=True)
            self.assertIn("warning:", proc.stderr, (name, proc.stderr))
            self.assertNotIn("error:", proc.stderr, name)
            self.assertTrue(self.called.exists(), f"{name} did not go on to cargo")
            self.called.unlink()

    def test_the_override_is_never_read_for_a_signed_build(self):
        key = pathlib.Path(self.tmp.name) / "k"
        key.write_text("x")
        cases = [("plugin-files", ("--key", str(key))),
                 ("plugin-files", ("--fingerprint", "00ff00ff00ff00ff")),
                 ("plugin-files7", ("--key", str(key)))]
        for name, args in cases:
            proc = self.run_script(name, *args, override=True)
            self.assertNotEqual(proc.returncode, 0, (name, args))
            self.assertIn("error:", proc.stderr, (name, args, proc.stderr))
            self.assertIn("never one with --", proc.stderr, (name, args))
            self.assertFalse(self.called.exists(), f"{name} {args} called cargo")


if __name__ == "__main__":
    unittest.main()
