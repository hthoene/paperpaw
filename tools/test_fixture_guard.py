"""Tests of fixture-guard.py (#244 safety net, recipe Q2).

    python3 -m unittest tools/test_fixture_guard.py
"""
import importlib.util
import pathlib
import subprocess
import tempfile
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("fg", HERE / "fixture-guard.py")
fg = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fg)

BASE = '''use paperpaw_sdk::Plugin;

const EXPECTED: u32 = 3;

struct Fixture;

impl Plugin for Fixture {
    fn id(&self) -> &str {
        "fixture"
    }
}

fn helper() -> u32 {
    7
}

#[test]
fn a_test() {
    let kernel = Kernel::new().plugin(Fixture);
    assert_eq!(helper(), EXPECTED);
}

#[test]
fn another() {}
'''

HEAD = '''use paperpaw_sdk::{Plugin, Other};

const EXPECTED: u32 = 4;

struct Fixture;

impl Plugin for Fixture {
    fn id(&self) -> &str {
        "changed inside the fixture"
    }
}

fn helper() -> u32 {
    8
}

#[test]
fn a_test() {
    let kernel = Kernel::new().native_plugin(Fixture);
    assert_eq!(helper(), EXPECTED);
}

#[test]
#[ignore = "not yet"]
fn another() {}
'''

SOURCE_BASE = '''pub fn real() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    #[test]
    fn t() {}
}
'''

SOURCE_HEAD = SOURCE_BASE.replace("    1\n", "    2\n")


class Repo:
    def __init__(self, files_base, files_head):
        self.dir = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.dir.name)
        self.run("init", "-q")
        self.run("config", "user.email", "t@example.org")
        self.run("config", "user.name", "t")
        self.base = self.commit(files_base)
        self.head = self.commit(files_head)

    def run(self, *args):
        return subprocess.run(["git", *args], cwd=self.root, check=True, text=True, stdout=subprocess.PIPE).stdout.strip()

    def commit(self, files):
        for name, text in files.items():
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        self.run("add", "-A")
        self.run("commit", "-q", "-m", "x")
        return self.run("rev-parse", "HEAD")


class Guard(unittest.TestCase):
    def listed(self, base, head, **kw):
        repo = Repo(base, head)
        with repo.dir:
            return fg.guard(repo.root, repo.base, repo.head, **kw)

    def test_fixture_blocks_and_builder_calls_are_silent_and_the_rest_is_listed(self):
        listed, builders = self.listed({"crates/x/tests/t.rs": BASE}, {"crates/x/tests/t.rs": HEAD})
        text = "\n".join(listed)
        self.assertIn("-const EXPECTED: u32 = 3;", text)          # a changed constant
        self.assertIn("+const EXPECTED: u32 = 4;", text)
        self.assertIn("-    7", text)                              # a changed helper
        self.assertIn("+    8", text)
        self.assertIn('+#[ignore = "not yet"]', text)              # a new ignore
        self.assertIn("number of `#[ignore` lines 0 -> 1", text)
        self.assertNotIn("changed inside the fixture", text)       # fixture block
        self.assertNotIn("Other", text)                            # the `use` line
        self.assertNotIn(".native_plugin(", text)                  # the builder-call pattern
        self.assertEqual(builders, {"crates/x/tests/t.rs": 1})

    def test_without_the_pattern_the_builder_line_is_listed(self):
        listed, _ = self.listed({"crates/x/tests/t.rs": BASE}, {"crates/x/tests/t.rs": HEAD}, builder_to=r"\.nothing\(")
        self.assertTrue(any(".plugin(Fixture)" in line for line in listed))

    def test_a_change_outside_the_test_module_of_a_source_file_is_shown(self):
        listed, _ = self.listed({"crates/x/src/lib.rs": SOURCE_BASE}, {"crates/x/src/lib.rs": SOURCE_HEAD})
        self.assertEqual(len(listed), 2)
        self.assertTrue(all("OUTSIDE cfg(test)" in line for line in listed), listed)

    def test_a_change_inside_the_test_module_of_a_source_file_is_a_test_change(self):
        head = SOURCE_BASE.replace("fn t() {}", "fn t() { assert!(true); }")
        listed, _ = self.listed({"crates/x/src/lib.rs": SOURCE_BASE}, {"crates/x/src/lib.rs": head})
        self.assertEqual(len(listed), 2)
        self.assertFalse(any("OUTSIDE" in line for line in listed), listed)

    def test_a_file_without_test_code_is_not_the_guards_business(self):
        listed, builders = self.listed({"crates/x/src/lib.rs": "pub fn a() {}\n"}, {"crates/x/src/lib.rs": "pub fn b() {}\n"})
        self.assertEqual((listed, builders), ([], {}))

    def test_an_unchanged_port_lists_nothing(self):
        listed, _ = self.listed({"crates/x/tests/t.rs": BASE, "README.md": "a\n"}, {"crates/x/tests/t.rs": BASE, "README.md": "b\n"})
        self.assertEqual(listed, [])

    def test_a_changed_should_panic_and_cfg_are_counted(self):
        base = "#[test]\n#[should_panic]\nfn a() {}\n"
        head = "#[test]\n#[cfg(feature = \"x\")]\nfn a() {}\n"
        listed, _ = self.listed({"crates/x/tests/t.rs": base}, {"crates/x/tests/t.rs": head})
        text = "\n".join(listed)
        self.assertIn("number of `#[should_panic` lines 1 -> 0", text)
        self.assertIn("number of `#[cfg` lines 0 -> 1", text)


class Masking(unittest.TestCase):
    def test_braces_in_strings_comments_and_chars_do_not_count(self):
        text = 'impl Plugin for A {\n    fn f() { let s = "}"; let c = \'}\'; /* } */ }\n    // }\n}\nfn after() {}\n'
        fixture, _, _ = fg.regions(text)
        self.assertEqual(sorted(fixture), [0, 1, 2, 3])


if __name__ == "__main__":
    unittest.main()
