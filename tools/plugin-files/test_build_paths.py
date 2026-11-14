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
import sys
import tempfile
import unittest
import unittest.mock

ROOT = pathlib.Path(__file__).resolve().parents[2]
# Each script with a plugin it knows (plugins.txt; the native entries).
SCRIPTS = {
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

    def run_script(self, name, *args, override=False, extra=None, target=True):
        env = {k: v for k, v in os.environ.items()
               # BASH_ENV: a profile it names may reset PATH, and then the real
               # cargo would build.
               if k not in ("RUSTFLAGS", "PAPERPAW_ALLOW_UNREMAPPED", "CARGO_ENCODED_RUSTFLAGS", "BASH_ENV")}
        env["PATH"] = f"{self.bin}:{env['PATH']}"
        if target:
            env["CARGO_TARGET_DIR"] = str(self.spaced)
        env.update(extra or {})
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
        cases = [("plugin-files7", ("--key", str(key)))]
        for name, args in cases:
            proc = self.run_script(name, *args, override=True)
            self.assertNotEqual(proc.returncode, 0, (name, args))
            self.assertIn("error:", proc.stderr, (name, args, proc.stderr))
            self.assertIn("never one with --", proc.stderr, (name, args))
            self.assertFalse(self.called.exists(), f"{name} {args} called cargo")

    def test_a_spaced_cargo_home_is_refused_too(self):
        for name in SCRIPTS:
            proc = self.run_script(name, target=False, extra={"CARGO_HOME": str(self.spaced)})
            self.assertNotEqual(proc.returncode, 0, name)
            self.assertIn(str(self.spaced), proc.stderr, (name, proc.stderr))
            self.assertIn("error:", proc.stderr, name)
            self.assertFalse(self.called.exists(), f"{name} called cargo")

    def test_a_spaced_checkout_is_refused_too(self):
        # The scripts copied into a folder with a space: `root` is two levels
        # above a script, so the checkout's path has the space.
        root = pathlib.Path(self.tmp.name) / "check out"
        for name, (script, plugin) in SCRIPTS.items():
            folder = root / "tools" / script.parent.name
            folder.mkdir(parents=True, exist_ok=True)
            for f in script.parent.iterdir():
                if f.is_file():
                    (folder / f.name).write_bytes(f.read_bytes())
            bins = folder / "src" / "bin"  # plugin-files7 lists its plugins here
            bins.mkdir(parents=True, exist_ok=True)
            (bins / f"{plugin}.rs").write_text("")
            (root / "plugins" / plugin).mkdir(parents=True, exist_ok=True)
            (root / "plugins" / plugin / "Cargo.toml").write_text(
                '[package]\nname = "x"\nversion = "1.0.0"\n')
            linker = root / "crates/paperpaw-native"
            linker.mkdir(parents=True, exist_ok=True)
            (linker / "paperpaw-plugin7.x").write_bytes(
                (ROOT / "crates/paperpaw-native/paperpaw-plugin7.x").read_bytes())
            env = {k: v for k, v in os.environ.items() if k not in ("RUSTFLAGS", "BASH_ENV")}
            env["PATH"] = f"{self.bin}:{env['PATH']}"
            env["CARGO_TARGET_DIR"] = str(pathlib.Path(self.tmp.name) / "target")
            proc = subprocess.run(
                ["bash", str(folder / "build.sh"), "x4", str(self.out), "--only", plugin],
                env=env, capture_output=True, text=True, timeout=120)
            self.assertNotEqual(proc.returncode, 0, name)
            self.assertIn("check out", proc.stderr, (name, proc.stderr))
            self.assertIn("error:", proc.stderr, name)
            self.assertFalse(self.called.exists(), f"{name} called cargo")


class StoreBaseline(unittest.TestCase):
    def test_versions_py_never_passes_the_override_to_build_sh(self):
        sys.path.insert(0, str(ROOT / "tools/plugin-files"))
        import versions
        seen = {}

        def fake_run(argv, **kwargs):
            seen.update(kwargs["env"])
            return subprocess.CompletedProcess(argv, 0, "", "")

        with unittest.mock.patch.dict(os.environ, {"PAPERPAW_ALLOW_UNREMAPPED": "1"}), \
                unittest.mock.patch.object(subprocess, "run", fake_run):
            versions.build_store(ROOT, pathlib.Path("/nonexistent"), "x4")
        self.assertTrue(seen)
        self.assertNotIn("PAPERPAW_ALLOW_UNREMAPPED", seen)
        self.assertNotIn("RUSTFLAGS", seen)


if __name__ == "__main__":
    unittest.main()
