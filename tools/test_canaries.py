"""Tests of the canary runner (tools/ci/canaries-244/canaries.py, #244 recipe Q3).

    python3 -m unittest tools/test_canaries.py

The end-to-end tests build a one-file crate in a temporary git repository: a
canary that a test catches, one that no test catches (a hole) and one that
does not compile (an ERROR, the run ends non-zero).
"""
import importlib.util
import io
import contextlib
import pathlib
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("canaries", HERE / "ci" / "canaries-244" / "canaries.py")
cn = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cn)

TEXT = """# a comment
== first (kernel)
# what it breaks
src/lib.rs
-    a + b
+    a - b

== second (stack)
# nothing catches it
src/lib.rs
-    a * 2 // unused
+    a * 3 // unused

== third (kernel)
# does not compile
src/lib.rs
-pub fn doubled(a: u32) -> u32 {
+pub fn doubled(a: u32) -> u32 { let
"""

LIB = """pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

pub fn unused(a: u32) -> u32 {
    a * 2 // unused
}

pub fn doubled(a: u32) -> u32 {
    a * 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds() {
        assert_eq!(add(1, 2), 3);
    }

    #[test]
    fn doubles() {
        assert_eq!(doubled(2), 4);
    }
}
"""


class Parsing(unittest.TestCase):
    def test_a_file_is_read_into_canaries(self):
        canaries = cn.parse(TEXT)
        self.assertEqual([(c.id, c.side) for c in canaries], [("first", "kernel"), ("second", "stack"), ("third", "kernel")])
        self.assertEqual(canaries[0].old, "    a + b")
        self.assertEqual(canaries[0].new, "    a - b")
        self.assertEqual(canaries[0].comment, "what it breaks")

    def test_a_block_without_its_new_line_is_refused(self):
        with self.assertRaises(ValueError):
            cn.parse("== x (kernel)\n# c\nsrc/lib.rs\n-old\n")

    def test_two_canaries_with_one_id_are_refused(self):
        block = "== x (kernel)\n# c\nf\n-a\n+b\n"
        with self.assertRaises(ValueError):
            cn.parse(block + block)

    def test_kernels_file_parses_if_it_is_there(self):
        path = HERE / "ci" / "canaries-244" / "canaries.txt"
        if path.exists():
            self.assertGreaterEqual(len(cn.parse(path.read_text(encoding="utf-8"))), 20)


class Failures(unittest.TestCase):
    def test_failing_tests_are_named_with_their_binary_and_a_crash_without_names_is_kept(self):
        tn = cn.load_test_names()
        output = "\n".join([
            "   Running tests/a.rs (target/debug/deps/a-0123456789abcdef)",
            "test x::y ... FAILED",
            "test z ... ok",
            "   Running tests/b.rs (target/debug/deps/b-0123456789abcdef)",
            "error: test failed, to rerun pass `-p pkg --test b`",
        ])
        names, crashed = cn.failing(output, {"target/debug/deps/a-0123456789abcdef": "pkg/a"}, tn)
        self.assertEqual(names, {"pkg/a\tx::y"})
        self.assertEqual(crashed, {"pkg/b"})


class Tree:
    def __init__(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.dir.name)
        (self.root / "src").mkdir()
        (self.root / "src/lib.rs").write_text(LIB)
        (self.root / "Cargo.toml").write_text('[package]\nname = "toy"\nversion = "0.1.0"\nedition = "2021"\n[workspace]\n')
        for args in (["init", "-q"], ["config", "user.email", "t@example.org"], ["config", "user.name", "t"], ["add", "-A"], ["commit", "-q", "-m", "x"]):
            subprocess.run(["git", *args], cwd=self.root, check=True, stdout=subprocess.PIPE)
        self.out = self.root / "out"
        self.canaries = self.root.parent / (self.root.name + "-canaries.txt")
        self.canaries.write_text(TEXT)


@unittest.skipUnless(shutil.which("cargo") and shutil.which("git"), "needs cargo and git")
class EndToEnd(unittest.TestCase):
    def run_runner(self, command, only=None):
        tree = Tree()
        try:
            argv = [command, "--tree", str(tree.root), "--file", str(tree.canaries), "--out", str(tree.out), "--set-args=--lib"]
            if only:
                argv += ["--only", only]
            saved = sys.argv
            sys.argv = ["canaries.py"] + argv
            text = io.StringIO()
            try:
                with contextlib.redirect_stdout(text):
                    code = cn.main()
            finally:
                sys.argv = saved
            fails = {p.name: p.read_text() for p in tree.out.glob("*") if p.is_file()} if tree.out.exists() else {}
            clean = subprocess.run(["git", "status", "--porcelain", "--untracked-files=no"], cwd=tree.root, text=True, stdout=subprocess.PIPE).stdout
            return code, text.getvalue(), fails, clean
        finally:
            tree.dir.cleanup()

    def test_caught_not_caught_and_build_error_end_to_end(self):
        code, text, files, clean = self.run_runner("run")
        lines = {l.split()[0]: l for l in text.splitlines() if l.startswith(("first", "second", "third"))}
        self.assertIn("CAUGHT", lines["first"])
        self.assertIn("toy/lib\ttests::adds", files["first.fails"])
        self.assertIn("NOT CAUGHT", lines["second"])
        self.assertEqual(files["second.fails"], "")
        self.assertIn("ERROR", lines["third"])
        self.assertIn("does not compile", files["third.error"])
        self.assertNotIn("third.fails", files)               # an error is not a catch
        self.assertEqual(code, 1, "a canary that does not build ends the run non-zero")
        self.assertEqual(clean, "", "the tree is restored")
        self.assertIn("HOLES", text)

    def test_a_syntax_error_alone_ends_non_zero(self):
        code, text, files, _ = self.run_runner("run", only="third")
        self.assertEqual(code, 1)
        self.assertIn("ERROR", text)
        self.assertNotIn("third.fails", files)

    def test_an_old_line_that_is_not_there_is_an_error(self):
        tree = Tree()
        try:
            tree.canaries.write_text(TEXT.replace("    a + b", "    a + c", 1))
            saved = sys.argv
            sys.argv = ["canaries.py", "run", "--tree", str(tree.root), "--file", str(tree.canaries), "--out", str(tree.out), "--set-args=--lib", "--only", "first"]
            text = io.StringIO()
            try:
                with contextlib.redirect_stdout(text):
                    code = cn.main()
            finally:
                sys.argv = saved
            self.assertEqual(code, 1)
            self.assertIn("not exactly once", text.getvalue())
        finally:
            tree.dir.cleanup()

    def test_apply_check_reports_each_canary(self):
        code, text, _, clean = self.run_runner("apply-check")
        self.assertEqual(code, 0, text)
        self.assertEqual(clean, "")


if __name__ == "__main__":
    unittest.main()
