"""Offline checks of publication resume, source verification and rate-limit waits."""

import argparse
import importlib.util
import json
import pathlib
import tempfile
import unittest
import urllib.error
from unittest import mock

SPEC = importlib.util.spec_from_file_location("publish_workspace", pathlib.Path(__file__).with_name("publish-workspace.py"))
publisher = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(publisher)

ITEMS = [{"name": "paperpaw-example", "version": "0.5.0-beta.8", "manifest_path": "Cargo.toml"}]


class RateLimits(unittest.TestCase):
    def test_crates_io_deadline_with_trailing_help_link(self):
        text = ("Please try again after Sun, 25 Oct 2026 23:20:43 GMT "
                "and see https://crates.io/docs/rate-limits for more details.")
        self.assertEqual(publisher.retry_delay(text, 0, now=1792970433), 15)

    def test_retry_after_seconds_and_date(self):
        self.assertEqual(publisher.retry_delay("Retry-After: 120\r\n", 0, now=0), 125)
        text = "Retry-After: Thu, 01 Jan 1970 00:01:00 GMT"
        self.assertEqual(publisher.retry_delay(text, 0, now=10), 55)

    def test_longest_hint_wins_and_past_hint_keeps_margin(self):
        self.assertEqual(publisher.retry_delay("Retry-After: 20\nRetry-After: 60", 0, now=0), 65)
        self.assertEqual(publisher.retry_delay("Retry-After: Thu, 01 Jan 1970 00:00:00 GMT", 0, now=10), 5)

    def test_fallback_is_bounded(self):
        self.assertEqual(publisher.retry_delay("no hint", 0), 30)
        self.assertEqual(publisher.retry_delay("no hint", 1), 60)
        self.assertEqual(publisher.retry_delay("no hint", 100), 900)

    def test_429_is_not_confused_with_compiler_line_number(self):
        self.assertTrue(publisher.rate_limited("failed to get a 200 OK response, got 429"))
        self.assertTrue(publisher.rate_limited("HTTP status code: 429"))
        self.assertFalse(publisher.rate_limited("error at src/lib.rs:429:12"))
        self.assertFalse(publisher.rate_limited("HTTP status code: 403"))

    @mock.patch.object(publisher.time, "sleep")
    @mock.patch.object(publisher.time, "time", side_effect=[0, 30, 60, 65])
    def test_wait_reports_progress_and_sleeps_at_most_thirty_seconds(self, clock, sleep):
        publisher.wait_until(65)
        self.assertEqual([call.args[0] for call in sleep.call_args_list], [30, 30, 5])


class Plans(unittest.TestCase):
    def test_journal_rejects_other_commit_or_versions(self):
        with tempfile.TemporaryDirectory() as temp:
            path = pathlib.Path(temp) / "journal.json"
            data = publisher.journal(path, "commit-a", ITEMS)
            publisher.save(path, data)
            self.assertEqual(publisher.journal(path, "commit-a", ITEMS), data)
            with self.assertRaisesRegex(ValueError, "another commit"):
                publisher.journal(path, "commit-b", ITEMS)
            with self.assertRaisesRegex(ValueError, "another commit"):
                publisher.journal(path, "commit-a", [ITEMS[0] | {"version": "0.5.0-beta.9"}])

    @mock.patch.object(publisher, "output", side_effect=["commit\n", "?? unknown-file\n"])
    def test_dirty_checkout_is_refused(self, output):
        with self.assertRaisesRegex(ValueError, "checkout must be clean"):
            publisher.clean_commit()

    @mock.patch.object(publisher, "output", return_value="different-commit\n")
    def test_head_change_during_wait_is_refused(self, output):
        with self.assertRaisesRegex(ValueError, "HEAD changed"):
            publisher.clean_commit("recorded-commit")

    @mock.patch.object(publisher, "output")
    def test_only_workspace_crates_allowed_on_crates_io_are_selected(self, output):
        packages = [
            {"id": "a", "name": "a", "publish": None},
            {"id": "b", "name": "b", "publish": []},
            {"id": "c", "name": "c", "publish": ["private"]},
            {"id": "d", "name": "d", "publish": ["crates-io"]},
            {"id": "outside", "name": "outside", "publish": None},
        ]
        output.return_value = json.dumps({"workspace_members": ["a", "b", "c", "d"], "packages": packages})
        self.assertEqual([p["name"] for p in publisher.packages()], ["a", "d"])

    def test_native_cargo_orders_packages_and_excludes_verified_versions(self):
        self.assertEqual(publisher.command(["existing"], False),
                         ["cargo", "publish", "--workspace", "--locked", "--dry-run", "--exclude", "existing"])
        self.assertNotIn("--dry-run", publisher.command([], True))

    @mock.patch.object(publisher.registry, "published_version", return_value={"yanked": False, "cksum": "checksum"})
    @mock.patch.object(publisher.registry, "archive_files", return_value={})
    @mock.patch.object(publisher.registry, "changed_files", return_value=["src/lib.rs"])
    @mock.patch.object(publisher, "output", return_value="src/lib.rs\n")
    def test_libraries_with_changed_published_sources_are_refused(self, output, changes, archive, entry):
        with self.assertRaisesRegex(ValueError, "differs from crates.io"):
            publisher.remaining(ITEMS, {})


class Resume(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = pathlib.Path(self.temp.name)
        (self.root / "Cargo.toml").write_text("[workspace]\n")
        self.options = argparse.Namespace(publish=True, dry_run=False, max_retries=8, journal=self.root / "journal.json")
        for name, value in [("ROOT", self.root)]:
            patch = mock.patch.object(publisher, name, value)
            patch.start()
            self.addCleanup(patch.stop)
        for name, kwargs in [("clean_commit", {"return_value": "commit"}),
                             ("packages", {"return_value": ITEMS}),
                             ("remaining", {}), ("run_cargo", {}), ("wait_until", {})]:
            patch = mock.patch.object(publisher, name, **kwargs)
            setattr(self, name, patch.start())
            self.addCleanup(patch.stop)

    def test_partial_success_is_rechecked_and_only_missing_crates_retry(self):
        self.remaining.side_effect = [(["one", "two"], []), (["two"], ["one"]), ([], ["one", "two"])]
        self.run_cargo.side_effect = [(101, "got 429\nRetry-After: 60"), (0, "uploaded")]
        publisher.execute(self.options)
        self.assertEqual(self.run_cargo.call_count, 2)
        self.assertEqual(self.run_cargo.call_args_list[1].args[0][-2:], ["--exclude", "one"])
        self.wait_until.assert_called_once()
        data = json.loads(self.options.journal.read_text())
        self.assertEqual(data["pending"], [])
        self.assertEqual(len(data["attempts"]), 2)

    def test_other_failures_stop_without_retry(self):
        self.remaining.return_value = (["one"], [])
        self.run_cargo.return_value = (101, "HTTP status code: 403")
        with self.assertRaisesRegex(ValueError, "No automatic retry"):
            publisher.execute(self.options)
        self.run_cargo.assert_called_once()
        self.wait_until.assert_not_called()

    def test_retry_limit_saves_server_deadline_for_later_resume(self):
        self.options.max_retries = 0
        self.remaining.return_value = (["one"], [])
        self.run_cargo.return_value = (101, "got 429\nRetry-After: 60")
        with self.assertRaisesRegex(ValueError, "retry limit reached"):
            publisher.execute(self.options)
        self.assertGreater(json.loads(self.options.journal.read_text())["retry_at"], 0)

    def test_rate_limits_with_progress_do_not_use_up_the_retry_limit(self):
        self.options.max_retries = 1
        # Each 429 run still published one more crate, so the run never stalls.
        self.remaining.side_effect = [(["a", "b", "c"], []), (["b", "c"], ["a"]),
                                      (["c"], ["a", "b"]), ([], ["a", "b", "c"])]
        self.run_cargo.return_value = (101, "got 429\nRetry-After: 60")
        publisher.execute(self.options)
        self.assertEqual(self.run_cargo.call_count, 3)

    def test_success_is_not_resubmitted_while_index_is_stale(self):
        self.remaining.side_effect = [(["one"], []), (["one"], []), ([], ["one"])]
        self.run_cargo.return_value = (0, "uploaded")
        publisher.execute(self.options)
        self.run_cargo.assert_called_once()
        self.wait_until.assert_called_once()

    def test_default_plan_never_starts_upload_or_dry_run(self):
        self.options.publish = False
        self.remaining.return_value = (["one"], [])
        publisher.execute(self.options)
        self.run_cargo.assert_not_called()

    def test_plan_does_not_erase_a_saved_publication_deadline(self):
        publisher.save(self.options.journal, {
            "commit": "commit", "packages": {ITEMS[0]["name"]: ITEMS[0]["version"]},
            "attempts": [], "retry_at": 9999999999,
        })
        self.options.publish = False
        self.remaining.return_value = (["one"], [])
        publisher.execute(self.options)
        self.wait_until.assert_not_called()
        self.assertEqual(json.loads(self.options.journal.read_text())["retry_at"], 9999999999)

    def test_network_failure_is_not_treated_as_a_missing_crate(self):
        self.remaining.side_effect = urllib.error.URLError("connection failed")
        with self.assertRaises(urllib.error.URLError):
            publisher.execute(self.options)
        self.run_cargo.assert_not_called()


if __name__ == "__main__":
    unittest.main()
