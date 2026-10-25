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

    def test_a_change_no_reader_sees_keeps_it(self):
        quiet = "test(stats): one more case\n\nPlugin-unchanged: stats, widgets"
        everything = "chore: rustfmt\n\nplugin-unchanged: all"
        other = "docs(widgets): a comment\n\nPlugin-unchanged: widgets"
        self.assertIsNone(versions.problem("stats", "1.0.0", "1.0.0", [quiet, everything]))
        self.assertIsNotNone(versions.problem("stats", "1.0.0", "1.0.0", [quiet, other]))

    def test_a_plugin_s_own_tags(self):
        tags = ["v0.5.0-beta.2", "vocabulary-v1.0.1", "reading-goal-v1.0.1", "reading-v2.0.0",
                "vocabulary-v1.0.2-beta.1", "vocabulary-vx"]
        self.assertEqual(versions.own_tags("vocabulary", tags),
                         ["vocabulary-v1.0.1", "vocabulary-v1.0.2-beta.1"])
        self.assertEqual(versions.own_tags("reading", tags), ["reading-v2.0.0"])


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

    def check(self):
        """Whether the check passes in the repository."""
        here = self.root / "tools" / "plugin-files"
        # As CI runs it: the release before from the tags, not unittest's
        # own arguments.
        with unittest.mock.patch.multiple(versions, ROOT=self.root, HERE=here), \
                unittest.mock.patch.object(sys, "argv", ["versions.py"]), \
                contextlib.redirect_stdout(io.StringIO()):
            try:
                versions.main()
            except SystemExit:
                return False
        return True

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


if __name__ == "__main__":
    unittest.main()
