"""tools/check-local.sh: a failed step makes the script fail; a pass needs every step."""

import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parent / "check-local.sh"


@unittest.skipUnless(shutil.which("cargo") and shutil.which("bash"), "needs cargo and bash")
class CheckLocal(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.repo = Path(temporary.name)
        (self.repo / "tools").mkdir()
        shutil.copy(SCRIPT, self.repo / "tools/check-local.sh")
        (self.repo / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/good", "crates/bad"]\nresolver = "2"\n')
        for name, test in (("good", "assert_eq!(1, 1);"), ("bad", "assert_eq!(1, 2);")):
            crate = self.repo / "crates" / name
            (crate / "src").mkdir(parents=True)
            (crate / "Cargo.toml").write_text(f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2021"\n')
            (crate / "src/lib.rs").write_text(f"#[test]\nfn it_works() {{\n    {test}\n}}\n")
        self.git("init", "-q", "-b", "main")
        self.git("add", ".")
        self.git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "base")
        self.git("branch", "base")

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.repo, check=True, stdout=subprocess.DEVNULL)

    def run_script(self, *args):
        env = dict(os.environ, CARGO_TARGET_DIR=str(self.repo / "target"), HOME=os.environ.get("HOME", "/"))
        return subprocess.run(
            ["bash", "tools/check-local.sh", "--base", "base", *args],
            cwd=self.repo, env=env, capture_output=True, text=True,
        )

    def test_passing_steps_exit_zero(self):
        run = self.run_script("--only", "fmt,test", "good")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("all 2 steps passed", run.stdout)

    def test_a_failing_test_exits_non_zero_and_is_named(self):
        run = self.run_script("--only", "fmt,test", "bad")
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertRegex(run.stdout, r"test\s+FAILED \(exit 101\)")
        self.assertRegex(run.stdout, r"fmt\s+ok")

    def test_a_compile_error_fails_and_later_steps_still_run(self):
        (self.repo / "crates/good/src/lib.rs").write_text("fn broken( {\n")
        run = self.run_script("--only", "test,fmt", "good")
        self.assertEqual(run.returncode, 1)
        self.assertRegex(run.stdout, r"test\s+FAILED")
        self.assertRegex(run.stdout, r"fmt\s+FAILED")  # unparsable source fails rustfmt too

    def test_a_step_that_cannot_run_is_a_failure(self):
        # No tools/changelog.py in this repository: python3 exits 2.
        run = self.run_script("--only", "changelog")
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertRegex(run.stdout, r"changelog\s+FAILED")

    def test_changed_packages_are_found_against_the_base(self):
        (self.repo / "crates/good/src/lib.rs").write_text("#[test]\nfn it_works() {}\n")
        run = self.run_script("--only", "test")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("packages: good", run.stdout)

    def test_nothing_selected_is_no_pass(self):
        run = self.run_script("--only", "nonsense")
        self.assertEqual(run.returncode, 2)

    def test_unknown_option_is_refused(self):
        self.assertEqual(self.run_script("--frobnicate").returncode, 2)


if __name__ == "__main__":
    unittest.main()
