"""Tests of versions.py: its rules, and the check in a repository made here.

    python3 -m unittest discover -s tools/plugin-files
"""

import contextlib
import io
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import unittest
import unittest.mock

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
import versions  # noqa: E402

WORKSPACE = '[workspace.package]\nversion = "0.5.0-beta.1"\n'


class Versions(unittest.TestCase):
    def test_semver_order(self):
        order = ["0.5.0-beta.1", "0.5.0-beta.2", "0.5.0-beta.10", "0.5.0-rc.1", "0.5.0", "1.0.0",
                 "1.0.1-beta.1", "1.0.1", "1.10.0"]
        self.assertEqual(sorted(reversed(order), key=versions.semver), order)

    def test_a_crate_of_the_workspace_s_version_has_it(self):
        self.assertEqual(versions.version_in('[package]\nversion.workspace = true\n', WORKSPACE),
                         "0.5.0-beta.1")
        self.assertEqual(versions.version_in('[package]\nversion = "1.0.0"\n', WORKSPACE), "1.0.0")

    def test_a_change_needs_a_newer_version(self):
        fix = "fix(vocabulary): Again waits its ten minutes"
        self.assertIsNotNone(versions.problem("vocabulary", "1.0.0", "1.0.0", [fix]))
        self.assertIsNone(versions.problem("vocabulary", "1.0.0", "1.0.1", [fix]))
        self.assertIsNone(versions.problem("vocabulary", "1.0.0", "1.0.0", []))
        # Never older than the release's.
        self.assertIsNotNone(versions.problem("vocabulary", "1.0.1", "1.0.0", []))

    def test_a_raised_plugin_interface_needs_a_newer_version_unchanged_or_not(self):
        self.assertIsNotNone(versions.problem("weather", "1.0.0", "1.0.0", [], raised=3))
        self.assertIsNone(versions.problem("weather", "1.0.0", "1.0.1", [], raised=3))
        quiet = "chore: a rename\n\nPlugin-unchanged: weather"
        self.assertIsNotNone(versions.problem("weather", "1.0.0", "1.0.0", [quiet], raised=3))

    def test_the_interface_is_read_from_native_rs(self):
        self.assertEqual(versions.interface_at("pub const INTERFACE: u16 = 3;\n"), 3)
        self.assertIsNone(versions.interface_at("nothing here"))
        self.assertEqual(versions.draft_at("pub const DRAFT: u16 = 2;\n"), 2)
        self.assertIsNone(versions.draft_at("pub const INTERFACE: u16 = 7;\n"))

    def test_a_new_draft_or_the_freeze_is_raised_like_an_interface(self):
        def raised(then, now):
            with tempfile.TemporaryDirectory() as root:
                path = pathlib.Path(root) / versions.NATIVE
                path.parent.mkdir(parents=True)
                path.write_text(now)
                with unittest.mock.patch.object(versions, "ROOT", pathlib.Path(root)), \
                        unittest.mock.patch.object(versions, "git", return_value=then):
                    return versions.interface_raised("v0.6.0-beta.3")
        six = "pub const INTERFACE: u16 = 6;\n"
        seven = "pub const INTERFACE: u16 = 7;\n"
        draft = lambda n: seven + f"pub const DRAFT: u16 = {n};\n"
        self.assertEqual(raised(six, draft(1)), "7 (draft 1)")
        self.assertEqual(raised(draft(1), draft(2)), "7 (draft 2)")
        self.assertEqual(raised(draft(2), seven), "7")
        self.assertIsNone(raised(draft(2), draft(2)))
        self.assertIsNone(raised(seven, seven))

    def test_the_interface_constants_must_stand_alone_on_their_line(self):
        for text in ['// pub const INTERFACE: u16 = 9;\n', '    pub const INTERFACE: u16 = 9;\n',
                     'pub const INTERFACE: u16 = 9; // x\n', 'const X: &str = "pub const INTERFACE: u16 = 9;";\n',
                     'pub(crate) const INTERFACE: u16 = 9;\n']:
            self.assertIsNone(versions.interface_at(text), text)
            self.assertIsNone(versions.draft_at(text.replace("INTERFACE", "DRAFT")), text)
        self.assertEqual(versions.interface_at("//\npub const INTERFACE: u16 = 7;\n"), 7)

    def test_a_plugin_s_raise_is_counted_from_the_build_that_carries_its_version(self):
        def raised(**tags):
            """`interface_raised` answers by the tag it is asked about."""
            with unittest.mock.patch.object(versions, "interface_raised", side_effect=tags.get) as asked:
                found = versions.raised_for({"v1": "1.0.0", "p-v1.0.1": "1.0.1"}, "1.0.1", "v1")
                return found, asked.call_args
        # Its own release carries the version: asked from there, the draft counted.
        self.assertEqual(raised(**{"v1": "7 (draft 1)"})[0], None)
        self.assertEqual(raised(**{"p-v1.0.1": "7 (draft 2)"})[0], "7 (draft 2)")
        self.assertEqual(raised(**{"p-v1.0.1": "8 (draft 1)"})[1].args, ("p-v1.0.1",))
        # The release itself carries it.
        with unittest.mock.patch.object(versions, "interface_raised", return_value="7 (draft 1)") as asked:
            self.assertEqual(versions.raised_for({"v1": "1.0.0"}, "1.0.0", "v1"), "7 (draft 1)")
            self.assertEqual(asked.call_args.args, ("v1",))

    def test_a_change_no_reader_sees_keeps_it(self):
        quiet = "test(stats): one more case\n\nPlugin-unchanged: stats, widgets"
        everything = "chore: rustfmt\n\nplugin-unchanged: all"
        other = "docs(widgets): a comment\n\nPlugin-unchanged: widgets"
        self.assertIsNone(versions.problem("stats", "1.0.0", "1.0.0", [quiet, everything]))
        self.assertIsNotNone(versions.problem("stats", "1.0.0", "1.0.0", [quiet, other]))

    def test_a_built_in_plugin_needs_a_newer_version_for_any_change(self):
        fix = "refactor(menu): rename"
        self.assertIsNotNone(versions.builtin_problem("menu", "1.0.2", "1.0.2", [fix]))
        self.assertIsNone(versions.builtin_problem("menu", "1.0.2", "1.0.3", [fix]))
        self.assertIsNone(versions.builtin_problem("menu", "1.0.2", "1.0.2", []))
        self.assertIsNotNone(versions.builtin_problem("menu", "1.0.2", "1.0.1", []))
        # A trailer does not excuse it: the published sources would differ.
        quiet = "test(menu): a case\n\nPlugin-unchanged: menu"
        self.assertIsNotNone(versions.builtin_problem("menu", "1.0.2", "1.0.2", [quiet]))

    def test_a_plugin_s_own_tags(self):
        tags = ["v0.5.0-beta.2", "vocabulary-v1.0.1", "reading-goal-v1.0.1", "reading-v2.0.0",
                "vocabulary-v1.0.2-beta.1", "vocabulary-vx"]
        self.assertEqual(versions.own_tags("vocabulary", tags),
                         ["vocabulary-v1.0.1", "vocabulary-v1.0.2-beta.1"])
        self.assertEqual(versions.own_tags("reading", tags), ["reading-v2.0.0"])


class BuiltFiles(unittest.TestCase):
    """The comparison of built store files: no build needed."""

    def files(self, folder, **plugins):
        """store files in `folder`: id (underscores for hyphens) -> (version, bytes)."""
        folder = pathlib.Path(folder)
        for name, (version, data) in plugins.items():
            (folder / f"{name.replace('_', '-')}-{version}-x4.ppn").write_bytes(data)
        return versions.store_files(folder, "x4")

    def test_files_are_found_by_their_names(self):
        with tempfile.TemporaryDirectory() as folder:
            found = self.files(folder, end_of_book=("1.0.0", b"a"), stats=("1.0.3-beta.1", b"b"))
            (pathlib.Path(folder) / "stats-1.0.3-x4-pro.ppn").write_bytes(b"other device")
            (pathlib.Path(folder) / "readme.txt").write_text("x")
            found = versions.store_files(folder, "x4")
            self.assertEqual({k: v[0] for k, v in found.items()},
                             {"end-of-book": "1.0.0", "stats": "1.0.3-beta.1"})

    def test_the_next_patch(self):
        self.assertEqual(versions.next_patch("1.0.4"), "1.0.5")
        self.assertEqual(versions.next_patch("1.9.9"), "1.9.10")
        self.assertIsNone(versions.next_patch("1.0.0-beta.2"))

    def test_other_bytes_under_the_same_version_are_wrong(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            then = self.files(a, kosync=("1.0.2", b"old"), stats=("1.0.0", b"same"),
                              widgets=("1.0.4", b"old"), gone=("1.0.0", b"x"))
            now = self.files(b, kosync=("1.0.2", b"new"), stats=("1.0.0", b"same"),
                             widgets=("1.0.5", b"new"), fresh=("1.0.0", b"y"))
            wrong, apart, new = versions.built_problems(then, now)
            self.assertEqual(wrong, [("kosync", "1.0.2")])
            self.assertEqual((apart, new), ([], ["fresh"]))

    def test_files_for_another_firmware_are_not_compared(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            then = self.files(a, kosync=("1.0.2", b"fp1|code"))
            now = self.files(b, kosync=("1.0.2", b"fp2|code"))
            identity = lambda path: path.read_bytes().split(b"|")[0]  # noqa: E731
            self.assertEqual(versions.built_problems(then, now, identity), ([], ["kosync"], []))
            self.assertEqual(versions.built_problems(then, now), ([("kosync", "1.0.2")], [], []))


class Repository(unittest.TestCase):
    """The check in a repository of one plugin: released with PaperPaw
    (`v0.5.0-beta.2`), then alone (`vocabulary-v1.0.1`, from a branch of
    the release merged back)."""

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.dir.name)
        self.env = {**os.environ, "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@t",
                    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@t"}
        self.git("init", "-q", "-b", "main")
        # These tiny repositories are deleted immediately. Detached Git
        # housekeeping can still write objects while cleanup removes them.
        self.git("config", "gc.auto", "0")
        self.git("config", "maintenance.auto", "false")
        self.write("tools/plugin-files/plugins.txt", "vocabulary reading\n")
        self.write("Cargo.toml", '[workspace.package]\nversion = "0.5.0-beta.2"\n')
        self.version("1.0.0")
        self.write("plugins/vocabulary/src/lib.rs", "// one\n")
        self.write(versions.NATIVE, "pub const INTERFACE: u16 = 6;\n")
        self.commit("release: 0.5.0-beta.2")
        self.git("tag", "v0.5.0-beta.2")
        # The fix alone, from the release's tag, merged back.
        self.git("switch", "-q", "-c", "fix", "v0.5.0-beta.2")
        self.write("plugins/vocabulary/src/lib.rs", "// two\n")
        self.version("1.0.1")
        self.commit("fix(vocabulary): Again waits")
        self.git("tag", "vocabulary-v1.0.1")
        self.git("switch", "-q", "main")
        self.git("merge", "-q", "--no-ff", "-m", "merge the fix", "fix")

    def tearDown(self):
        self.dir.cleanup()

    def git(self, *args):
        subprocess.run(["git", *args], cwd=self.root, env=self.env, check=True, capture_output=True)

    def write(self, path, text):
        path = self.root / path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def version(self, version):
        self.write("plugins/vocabulary/Cargo.toml", f'[package]\nname = "v"\nversion = "{version}"\n')

    def commit(self, message):
        self.git("add", "-A")
        self.git("commit", "-q", "-m", message)

    def check(self, *flags):
        """Whether the check passes in the repository (`flags`: versions.py's)."""
        here = self.root / "tools" / "plugin-files"
        # As CI runs it: the release before from the tags, not unittest's
        # own arguments.
        with unittest.mock.patch.multiple(versions, ROOT=self.root, HERE=here), \
                unittest.mock.patch.object(sys, "argv", ["versions.py", *flags]), \
                contextlib.redirect_stdout(io.StringIO()):
            try:
                versions.main()
            except SystemExit:
                return False
        return True

    def builtin(self, version, source="// one\n", publish=True):
        extra = "" if publish else "publish = false\n"
        self.write("plugins/menu/Cargo.toml",
                   f'[package]\nname = "m"\nversion = "{version}"\n{extra}')
        self.write("plugins/menu/src/lib.rs", source)

    def built_in_release(self, **kept):
        """A release with a built-in plugin, `menu` (not in plugins.txt)."""
        self.builtin("1.0.2", **kept)
        self.commit("menu is built in")
        self.git("tag", "v0.5.0-beta.3")

    def test_a_changed_built_in_plugin_warns_and_fails_for_the_release(self):
        self.built_in_release()
        self.assertTrue(self.check())
        self.assertTrue(self.check("--release"))
        self.write("plugins/menu/src/lib.rs", "// two\n")
        self.commit("refactor(menu): rename\n\nPlugin-unchanged: menu")
        # A warning in CI, an error in the step before the release.
        self.assertTrue(self.check())
        self.assertFalse(self.check("--release"))
        self.builtin("1.0.3", source="// two\n")
        self.commit("menu 1.0.3")
        self.assertTrue(self.check("--release"))

    def test_a_built_in_plugin_changed_and_changed_back_is_left_alone(self):
        self.built_in_release()
        self.write("plugins/menu/src/lib.rs", "// two\n")
        self.commit("refactor(menu): rename")
        self.write("plugins/menu/src/lib.rs", "// one\n")
        self.commit("revert it")
        self.assertTrue(self.check("--release"))

    def test_a_built_in_plugin_that_is_not_published_is_left_alone(self):
        self.built_in_release(publish=False)
        self.write("plugins/menu/src/lib.rs", "// two\n")
        self.commit("refactor(menu): rename")
        self.assertTrue(self.check("--release"))

    def test_a_built_in_plugin_new_since_the_release_is_left_alone(self):
        self.builtin("1.0.0")
        self.commit("a new built-in plugin")
        self.assertTrue(self.check("--release"))

    def test_the_fix_alone_merged_back_passes(self):
        self.assertTrue(self.check())

    def test_commits_do_not_start_housekeeping_even_if_enabled_globally(self):
        with tempfile.TemporaryDirectory() as folder:
            folder = pathlib.Path(folder)
            config = folder / "gitconfig"
            config.write_text("[gc]\n\tauto = 1\n[maintenance]\n\tauto = true\n")
            trace = folder / "trace.json"
            self.env.update(GIT_CONFIG_GLOBAL=str(config), GIT_TRACE2_EVENT=str(trace))
            for index in range(3):
                self.write("plugins/vocabulary/src/lib.rs", f"// commit {index}\n")
                self.commit(f"change {index}")
            events = [json.loads(line) for line in trace.read_text(encoding="utf-8").splitlines()]
            housekeeping = [event["argv"] for event in events
                            if event["event"] == "child_start"
                            and any(arg in ("gc", "maintenance") for arg in event["argv"])]
            self.assertEqual(housekeeping, [], "cleanup must not race detached Git work")

    def test_a_change_after_its_own_release_needs_a_newer_version(self):
        self.write("plugins/vocabulary/src/lib.rs", "// three\n")
        self.commit("fix(vocabulary): another")
        # 1.0.1 is newer than the PaperPaw release's 1.0.0, but not than
        # its own release's.
        self.assertFalse(self.check())
        self.version("1.0.2")
        self.commit("vocabulary 1.0.2")
        self.assertTrue(self.check())

    def test_a_change_no_reader_sees_after_its_own_release_keeps_it(self):
        self.write("plugins/vocabulary/src/lib.rs", "// three\n")
        self.commit("test(vocabulary): a comment\n\nPlugin-unchanged: vocabulary")
        self.assertTrue(self.check())

    def test_the_fix_alone_picked_over_passes(self):
        # The fix on main as a commit of its own, its tag beside it.
        self.git("reset", "-q", "--hard", "HEAD^1")
        self.git("cherry-pick", "vocabulary-v1.0.1")
        self.assertTrue(self.check())
        # A change after it still needs a newer version.
        self.write("plugins/vocabulary/src/lib.rs", "// three\n")
        self.commit("fix(vocabulary): another")
        self.assertFalse(self.check())

    def raise_to(self, interface, draft=None):
        self.write(versions.NATIVE, f"pub const INTERFACE: u16 = {interface};\n"
                   + ("" if draft is None else f"pub const DRAFT: u16 = {draft};\n"))

    def test_one_bump_after_a_raise_and_none_for_a_draft_after_it(self):
        # The raise: vocabulary (1.0.1, its own release) needs a bump, and the trailer is no excuse.
        self.raise_to(7, 1)
        self.commit("interface: raise to 7, draft 1\n\nPlugin-unchanged: all")
        self.assertFalse(self.check())
        self.version("1.0.2")
        self.commit("vocabulary 1.0.2 for interface 7")
        self.assertTrue(self.check())
        # A draft re-bless between releases asks for no further bump.
        self.raise_to(7, 2)
        self.commit("interface: draft 2")
        self.assertTrue(self.check())
        # Still unreleased, the one bump covers a further raise as well: versions are
        # held to the release, not to each raise.
        self.raise_to(8, 1)
        self.commit("interface: raise to 8")
        self.assertTrue(self.check())

    def test_a_plugin_released_alone_at_the_current_draft_is_not_asked_again(self):
        self.raise_to(7, 1)
        self.version("1.0.2")
        self.commit("interface: raise to 7, draft 1; vocabulary 1.0.2")
        self.git("tag", "vocabulary-v1.0.2")
        # Its newest release is built at interface 7 draft 1: nothing is owed, though the
        # PaperPaw release before it was at 6.
        self.assertTrue(self.check())

    def test_a_plugin_released_alone_at_an_older_draft_owes_one_bump(self):
        self.raise_to(7, 1)
        self.version("1.0.2")
        self.commit("interface: raise to 7, draft 1; vocabulary 1.0.2")
        self.git("tag", "vocabulary-v1.0.2")
        self.assertTrue(self.check())
        # Draft 2: beta drafts can reach devices, so the draft 1 build is stale.
        self.raise_to(7, 2)
        self.commit("interface: draft 2\n\nPlugin-unchanged: all")
        self.assertFalse(self.check())
        self.version("1.0.3")
        self.commit("vocabulary 1.0.3 for draft 2")
        self.assertTrue(self.check())
        # Released at draft 2, it owes nothing more until the interface moves again.
        self.git("tag", "vocabulary-v1.0.3")
        self.assertTrue(self.check())
        self.raise_to(8, 1)
        self.commit("interface: raise to 8")
        self.assertFalse(self.check())

    def test_a_version_older_than_one_published_fails(self):
        # The fix released alone, not merged back yet.
        self.git("reset", "-q", "--hard", "HEAD^1")
        self.assertFalse(self.check())

    def data(self, version):
        self.write("tools/plugin-files/data.toml",
                   f'[fonts-extra]\nversion = "{version}"\nbuild = "tools/extra.py"\n'
                   'inputs = ["fonts/extra.ttf"]\n')

    def test_a_data_plugin_whose_inputs_changed_needs_a_newer_version(self):
        self.data("1.0.0")
        self.write("tools/extra.py", "# one\n")
        self.write("fonts/extra.ttf", "one")
        self.commit("data plugin")
        self.git("tag", "v0.5.0-beta.3")
        self.assertTrue(self.check())
        self.write("fonts/extra.ttf", "two")
        self.commit("a better font")
        self.assertFalse(self.check())
        self.data("1.0.1")
        self.commit("fonts-extra 1.0.1")
        self.assertTrue(self.check())

    def test_a_commit_listed_as_unchanged_needs_no_newer_version(self):
        self.data("1.0.0")
        self.write("tools/extra.py", "# one\n")
        self.write("fonts/extra.ttf", "one")
        self.commit("data plugin")
        self.git("tag", "v0.5.0-beta.3")
        self.write("fonts/extra.ttf", "two")
        self.commit("a font change with the same output")
        self.assertFalse(self.check())
        commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=self.root, text=True,
                                capture_output=True).stdout.strip()
        # Another plugin's name, or a too short hash, does not count.
        self.write("tools/plugin-files/unchanged.txt", f"{commit[:12]} other  # proof\n")
        self.commit("list it for another plugin")
        self.assertFalse(self.check())
        self.write("tools/plugin-files/unchanged.txt", f"{commit[:12]} other, fonts-extra  # proof\n")
        self.commit("list it")
        self.assertTrue(self.check())
        # A later change is still held to the rule.
        self.write("fonts/extra.ttf", "three")
        self.commit("another change")
        self.assertFalse(self.check())

    def test_a_data_plugin_released_alone_is_held_to_its_own_version(self):
        self.data("1.0.0")
        self.write("tools/extra.py", "# one\n")
        self.write("fonts/extra.ttf", "one")
        self.commit("data plugin")
        self.git("tag", "v0.5.0-beta.3")
        # A fix released alone (store-plugin.yml), on this branch.
        self.write("fonts/extra.ttf", "two")
        self.data("1.0.1")
        self.commit("fonts-extra 1.0.1")
        self.git("tag", "fonts-extra-v1.0.1")
        self.assertTrue(self.check())
        # Changed after its own release: 1.0.1 again will not do.
        self.write("fonts/extra.ttf", "three")
        self.commit("a better font again")
        self.assertFalse(self.check())
        self.data("1.0.2")
        self.commit("fonts-extra 1.0.2")
        self.assertTrue(self.check())
        # Older than its own release: refused.
        self.data("1.0.0")
        self.commit("back")
        self.assertFalse(self.check())


    BUILD = """#!/usr/bin/env bash
# A fake build.sh: each plugin's file holds its source, the crate it depends on
# (crates/dep: no part of plugins/<id>/) and the firmware's name (fingerprint.txt).
set -e
device=$1; out=$2; root="$(cd "$(dirname "$0")/../.." && pwd)"
mkdir -p "$out"
for id in $(awk 'NF { print $1 }' "$root/tools/plugin-files/plugins.txt"); do
  [ -f "$root/plugins/$id/Cargo.toml" ] || continue
  v=$(sed -n 's/^version = "\\(.*\\)"/\\1/p' "$root/plugins/$id/Cargo.toml")
  { cat "$root/fingerprint.txt"; cat "$root/plugins/$id/src/lib.rs" "$root/crates/dep/lib.rs"; } \\
    > "$out/$id-$v-$device.ppn"
done
"""
    SIGN = """#!/usr/bin/env bash
# A fake paperpaw-sign: `plugin <file>` says the firmware the file's first line names.
echo "fingerprint: $(head -1 "$2")"; echo "interface: 6"; echo "compiler: abc"
"""

    def released_with_a_dependency(self):
        """A release (`v0.5.0-beta.3`) whose store files are built by a fake build.sh from
        the plugin and a crate it depends on."""
        self.write("tools/plugin-files7/build.sh", self.BUILD)
        self.write("fingerprint.txt", "fp1\n")
        self.write("crates/dep/lib.rs", "// the SDK, one\n")
        self.commit("a dependency and a builder")
        self.git("tag", "v0.5.0-beta.3")
        self.sign = self.root / "fake-sign"
        self.sign.write_text(self.SIGN, encoding="utf-8")
        self.sign.chmod(0o755)
        self.cache = tempfile.TemporaryDirectory()
        self.addCleanup(self.cache.cleanup)
        env = unittest.mock.patch.dict(os.environ, {"PAPERPAW_STORE_BASELINE": self.cache.name})
        env.start()
        self.addCleanup(env.stop)

    def bytes_check(self):
        return self.check("--release", "--bytes", "--sign", str(self.sign))

    def test_a_change_in_a_dependency_needs_a_newer_version(self):
        self.released_with_a_dependency()
        self.assertTrue(self.bytes_check())
        self.write("crates/dep/lib.rs", "// the SDK, two\n")
        self.commit("sdk: a change no plugins/<id>/ shows")
        # The commit check cannot see it; the built files can.
        self.assertTrue(self.check("--release"))
        self.assertFalse(self.bytes_check())
        self.version("1.0.2")
        self.commit("vocabulary 1.0.2")
        self.assertTrue(self.bytes_check())

    def test_a_change_that_leaves_the_files_alone_needs_no_version(self):
        self.released_with_a_dependency()
        self.write("README.md", "docs\n")
        self.commit("docs")
        self.assertTrue(self.bytes_check())

    def test_files_built_for_another_firmware_are_left_to_the_version_rules(self):
        self.released_with_a_dependency()
        self.write("fingerprint.txt", "fp2\n")
        self.commit("the firmware's fingerprint changed")
        self.assertTrue(self.bytes_check())

    def test_the_release_s_build_is_kept_and_not_made_twice(self):
        self.released_with_a_dependency()
        built = []

        def build(root, out, device, target=None):
            built.append(device)
            versions.build_store(root, out, device, target)

        with unittest.mock.patch.object(versions, "ROOT", self.root):
            first = versions.baseline_files("v0.5.0-beta.3", "x4", build)
            second = versions.baseline_files("v0.5.0-beta.3", "x4", build)
        self.assertEqual((first, built), (second, ["x4"]))
        self.assertEqual(sorted(versions.store_files(first, "x4")), ["vocabulary"])
        # No worktree is left behind in the repository.
        listed = subprocess.run(["git", "worktree", "list"], cwd=self.root, capture_output=True,
                                text=True).stdout.splitlines()
        self.assertEqual(len(listed), 1)


if __name__ == "__main__":
    unittest.main()
