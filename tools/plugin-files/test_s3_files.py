"""The S3 devices' plugin files (target 9, #263): the generated metadata, build.sh's refusal of a
stale tree, and the workflows that build, check, sign and collect the files for all three devices."""

import importlib.util
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "tools/plugin-files7/s3-metadata.py"
BUILD = ROOT / "tools/plugin-files7/build.sh"
WORKFLOWS = ROOT / ".github/workflows"


def module():
    spec = importlib.util.spec_from_file_location("s3_metadata", SCRIPT)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def tree(root: Path, ids=("about",)):
    """A small repository: the generator, the schema, build.sh and the given plugins' metadata."""
    for rel in ("abi/abi.json", "tools/plugin-files7/build.sh", "tools/plugin-files7/s3-metadata.py"):
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(ROOT / rel, root / rel)
    # The generator and the modules it imports (tools/abi*.py).
    for module_file in (ROOT / "tools").glob("abi[-_]*.py"):
        shutil.copy(module_file, root / "tools" / module_file.name)
    (root / "tools/plugin-files7/src/bin").mkdir(parents=True, exist_ok=True)
    for id in ids:
        (root / f"tools/plugin-files7/src/bin/{id}.rs").write_text("fn main() {}\n")
        folder = root / "plugins" / id
        (folder / "native").mkdir(parents=True)
        shutil.copy(ROOT / f"plugins/{id}/native/metadata.json", folder / "native/metadata.json")
        (folder / "Cargo.toml").write_text(f'[package]\nname = "paperpaw-plugin-{id}"\nversion = "1.0.0"\n')
    return root


class GeneratedMetadata(unittest.TestCase):
    def test_every_plugin_has_the_s3_record_the_generator_writes(self):
        done = subprocess.run([sys.executable, str(SCRIPT), "--check"], capture_output=True, text=True)
        self.assertEqual(done.returncode, 0, done.stdout + done.stderr)

    def test_the_record_is_the_x4s_for_the_xtensa_profile_and_nothing_else(self):
        mod = module()
        with tempfile.TemporaryDirectory() as tmp:
            data, text = mod.generate(ROOT / "plugins/about/native", Path(tmp))
        self.assertIn(b"esp32s3-xtensa-lx7-ilp32-windowed-le", data)
        self.assertNotIn(b"esp32c3-riscv32imc", data)
        x4 = (ROOT / "plugins/about/native/metadata.bin").read_bytes()
        self.assertEqual(len(data) - len(x4), len("esp32s3-xtensa-lx7-ilp32-windowed-le") - len("esp32c3-riscv32imc-ilp32-soft-le"))

    def test_a_changed_x4_record_makes_the_check_fail_until_regenerated(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(Path(tmp))
            gen = root / "tools/plugin-files7/s3-metadata.py"
            run = lambda *a: subprocess.run([sys.executable, str(gen), *a], capture_output=True, text=True)
            self.assertEqual(run("--check").returncode, 1, "no s3 files yet")
            self.assertEqual(run().returncode, 0)
            self.assertEqual(run("--check").returncode, 0)
            meta = root / "plugins/about/native/metadata.json"
            record = json.loads(meta.read_text())
            record["services"] = record["services"][:-1]  # the plugin declares one service less
            meta.write_text(json.dumps(record, indent=2) + "\n")
            stale = run("--check")
            self.assertEqual(stale.returncode, 1, stale.stdout + stale.stderr)
            self.assertIn("about", stale.stdout)
            self.assertEqual(run().returncode, 0)
            self.assertEqual(run("--check").returncode, 0)


class BuildScript(unittest.TestCase):
    def test_an_s3_build_with_stale_metadata_is_a_broken_tree_not_pending_or_skipped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = tree(Path(tmp))
            (root / "tools/plugin-files7/complete.txt").write_text("")
            done = subprocess.run(
                ["bash", str(root / "tools/plugin-files7/build.sh"), "x4-pro", str(Path(tmp) / "out")],
                capture_output=True, text=True,
            )
            self.assertEqual(done.returncode, 3, done.stdout + done.stderr)
            self.assertIn("metadata.s3", done.stderr)
            self.assertNotIn("PENDING", done.stderr)

    def test_the_script_no_longer_has_a_pending_exit(self):
        text = BUILD.read_text()
        self.assertNotIn("PENDING", text)
        self.assertNotIn("pack=0", text)
        self.assertIn("chip=9", text)


class Workflows(unittest.TestCase):
    def read(self, name):
        return (WORKFLOWS / name).read_text()

    def test_no_workflow_tolerates_the_old_pending_exit(self):
        for name in ("ci.yml", "release.yml", "store-plugin.yml"):
            text = self.read(name)
            self.assertNotRegex(text, r"rc\s*=\s*4", name)
            self.assertNotIn("$rc = 4", text, name)

    def test_the_release_builds_and_signs_the_files_of_every_device_but_the_x3(self):
        text = self.read("release.yml")
        self.assertIn('tools/plugin-files7/build.sh "$DEVICE" dist/plugins', text)
        step = text.split("- name: Sign the store's first-party plugins", 1)[1].split("env:", 1)[0]
        self.assertIn("matrix.device != 'x3'", step)

    def test_ci_builds_packs_and_signs_the_s3_files_too(self):
        text = self.read("ci.yml")
        step = text.split("- name: The store's first-party plugins fit the firmware", 1)[1].split("- name:", 1)[0]
        self.assertIn("matrix.device != 'x3'", step)
        self.assertIn('plugin-files7/build.sh "$DEVICE"', step)

    def test_the_store_workflow_checks_chip_9_and_collects_three_files_per_plugin(self):
        text = self.read("store-plugin.yml")
        self.assertGreaterEqual(len(re.findall(r"for device in x4 x4-pro x4-classic; do", text)), 3, "check, sign, collect")
        self.assertIn('[[ "$device" == x4 ]] && chip=5', text)
        self.assertIn("want=$(( 3 * $(jq", text)
        self.assertNotIn("for device in x4; do", text)


if __name__ == "__main__":
    unittest.main()
