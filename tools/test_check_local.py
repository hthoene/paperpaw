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
        (self.repo / ".gitignore").write_text("target/\n")
        separate = self.repo / "shells/x"
        (separate / "src").mkdir(parents=True)
        (separate / "Cargo.toml").write_text(
            '[package]\nname = "x"\nversion = "0.1.0"\nedition = "2021"\n[workspace]\n')
        (separate / "src/lib.rs").write_text("")
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

    def test_a_copy_outside_the_worktree_is_refused(self):
        copy = Path(tempfile.mkdtemp()) / "check-local.sh"
        shutil.copy(self.repo / "tools/check-local.sh", copy)
        self.addCleanup(shutil.rmtree, copy.parent, True)
        env = dict(os.environ, CARGO_TARGET_DIR=str(self.repo / "target"))
        run = subprocess.run(
            ["bash", str(copy), "--base", "base", "--only", "test", "bad"],
            cwd=self.repo, env=env, capture_output=True, text=True,
        )
        self.assertEqual(run.returncode, 2, run.stdout + run.stderr)
        self.assertIn("not tools/check-local.sh of the worktree", run.stderr)
        self.assertNotIn("== test", run.stdout)

    def test_the_script_of_another_worktree_is_refused(self):
        other = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, other, True)
        (other / "tools").mkdir()
        shutil.copy(self.repo / "tools/check-local.sh", other / "tools/check-local.sh")
        run = subprocess.run(
            ["bash", str(other / "tools/check-local.sh"), "--only", "test", "good"],
            cwd=self.repo, capture_output=True, text=True,
        )
        self.assertEqual(run.returncode, 2, run.stdout + run.stderr)

    def test_outside_a_git_worktree_it_is_refused(self):
        run = subprocess.run(
            ["bash", str(self.repo / "tools/check-local.sh"), "--only", "test", "good"],
            cwd=tempfile.gettempdir(), capture_output=True, text=True,
        )
        self.assertEqual(run.returncode, 2, run.stdout + run.stderr)

    def test_a_workspace_whose_metadata_is_far_beyond_one_argument_still_detects_packages(self):
        members = ['"crates/good"', '"crates/bad"']
        for number in range(400):
            crate = self.repo / "crates" / f"many{number}"
            (crate / "src").mkdir(parents=True)
            (crate / "Cargo.toml").write_text(
                f'[package]\nname = "many{number}"\nversion = "0.1.0"\nedition = "2021"\n'
                f'description = "{"long description " * 20}"\n')
            (crate / "src/lib.rs").write_text("")
            members.append(f'"crates/many{number}"')
        (self.repo / "Cargo.toml").write_text(f'[workspace]\nmembers = [{", ".join(members)}]\nresolver = "2"\n')
        self.git("add", ".")
        self.git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "many")
        self.git("branch", "-f", "base")
        env = dict(os.environ)
        metadata = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=self.repo,
                                  env=env, capture_output=True, text=True, check=True).stdout
        self.assertGreater(len(metadata), 200 * 1024)
        (self.repo / "crates/many7/src/lib.rs").write_text("pub fn changed() {}\n")
        run = self.run_script("--only", "fmt")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("packages: many7", run.stdout)

    def test_a_missing_base_is_an_error_not_an_empty_package_set(self):
        run = self.run_script("--base", "origin/nowhere", "--only", "test")
        self.assertEqual(run.returncode, 2, run.stdout + run.stderr)
        self.assertIn("not found", run.stderr)

    def test_a_missing_base_with_named_crates_only_warns(self):
        run = self.run_script("--base", "origin/nowhere", "--only", "test", "good")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("WARNING: base 'origin/nowhere' not found", run.stdout)

    def test_a_change_in_a_separate_workspace_is_listed_as_not_checked(self):
        (self.repo / "shells/x/src/lib.rs").write_text("pub fn changed() {}\n")
        (self.repo / "crates/good/src/lib.rs").write_text("#[test]\nfn it_works() {}\n")
        run = self.run_script("--only", "test")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertIn("packages: good", run.stdout)
        summary = run.stdout.split("check-local summary:")[1]
        self.assertIn("NOT CHECKED: shells/x/ (separate workspace", summary)

    def test_a_badly_formatted_firmware_file_fails_fmt_shells(self):
        shell = self.repo / "shells/xteink/src"
        shell.mkdir(parents=True)
        (shell / "main.rs").write_text("fn main() {}\n")
        (self.repo / "shells/xteink/Cargo.toml").write_text(
            '[package]\nname = "xteink"\nversion = "0.1.0"\nedition = "2024"\n[workspace]\n')
        self.git("add", ".")
        self.git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "shell")
        self.git("branch", "-f", "base")
        (shell / "main.rs").write_text("fn   main( ) {  }\n")
        run = self.run_script("--only", "fmt-shells")
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertRegex(run.stdout, r"fmt-shells\s+FAILED")
        (shell / "main.rs").write_text("fn main() {}\n\nfn more() {}\n")
        run = self.run_script("--only", "fmt-shells")
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertRegex(run.stdout, r"fmt-shells\s+ok")

    def test_a_badly_formatted_fuzz_file_fails_fmt_fuzz(self):
        fuzz = self.repo / "fuzz/fuzz_targets"
        fuzz.mkdir(parents=True)
        (fuzz / "t.rs").write_text("fn t() {}\n")
        self.git("add", ".")
        self.git("-c", "user.name=t", "-c", "user.email=t@t", "commit", "-q", "-m", "fuzz")
        self.git("branch", "-f", "base")
        (fuzz / "t.rs").write_text("fn   t( ) {  }\n")
        run = self.run_script("--only", "fmt-fuzz")
        self.assertEqual(run.returncode, 1, run.stdout + run.stderr)
        self.assertRegex(run.stdout, r"fmt-fuzz\s+FAILED")

    def test_unchanged_shells_and_fuzz_are_listed_as_skipped(self):
        (self.repo / "crates/good/src/lib.rs").write_text("#[test]\nfn it_works() {}\n")
        run = self.run_script("--only", "fmt")
        summary = run.stdout.split("check-local summary:")[1]
        self.assertIn("SKIPPED: fmt-shells (no changed .rs file", summary)
        self.assertIn("SKIPPED: fmt-fuzz (no changed .rs file", summary)

    def test_skipped_steps_are_named_in_the_summary(self):
        run = self.run_script("--only", "test", "good")
        summary = run.stdout.split("check-local summary:")[1]
        self.assertIn("SKIPPED: fmt (left out by --only/--skip)", summary)
        self.assertIn("SKIPPED: clippy (left out by --only/--skip)", summary)

    def test_package_steps_without_a_package_are_skipped_and_said_so(self):
        (self.repo / "README.md").write_text("only docs changed\n")
        run = self.run_script("--only", "test,fmt")
        self.assertEqual(run.returncode, 2)  # nothing ran
        self.assertIn("SKIPPED: test (no package changed or named)", run.stdout)

    def test_changed_crates_without_a_changelog_fragment_warn(self):
        (self.repo / "crates/good/src/lib.rs").write_text("#[test]\nfn it_works() {}\n")
        run = self.run_script("--only", "test")
        self.assertIn("WARNING: crates changed but no new changelog.d fragment", run.stdout)
        (self.repo / "changelog.d").mkdir()
        (self.repo / "changelog.d/228-x.md").write_text("### Fixed\n\n- x\n")
        run = self.run_script("--only", "test")
        self.assertNotIn("no new changelog.d fragment", run.stdout)

    def test_an_unknown_step_name_is_refused(self):
        self.assertEqual(self.run_script("--only", "tset").returncode, 2)
        self.assertEqual(self.run_script("--skip", "frobnicate").returncode, 2)

    def test_nothing_selected_is_no_pass(self):
        run = self.run_script("--only", "test")
        self.assertEqual(run.returncode, 2)  # no package changed: test is skipped

    def test_unknown_option_is_refused(self):
        self.assertEqual(self.run_script("--frobnicate").returncode, 2)


if __name__ == "__main__":
    unittest.main()
