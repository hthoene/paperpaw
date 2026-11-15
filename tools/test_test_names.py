"""Tests of test-names.py (#244 safety net, recipe Q1).

    python3 -m unittest tools/test_test_names.py
"""
import importlib.util
import json
import pathlib
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("tn", HERE / "test-names.py")
tn = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tn)

CI = """
jobs:
  test:
    steps:
      - run: cargo test --no-fail-fast --workspace
      # - run: cargo test --commented
      - run: cargo test --no-fail-fast -p a -p b --features x,y --test t1 --test t2
      - name: two lines
        run: |
          cargo +1.88 test --manifest-path tools/bt/Cargo.toml --locked
          PAPERPAW_X=1 cargo test -p paperpaw-text --lib linebreak
          cargo test -p core --test native_plugin -- --ignored
          cargo test -q --release -p x4 --test update_path --no-run
          cargo test --no-fail-fast -p a -p b --features x,y --test t1 --test t2
      - run: cargo test -p app --test partial_redraw -- --exact table_of_contents
      - run: cargo clippy --workspace --all-targets
"""


class Sets(unittest.TestCase):
    def test_every_cargo_test_line_is_a_set_once_and_comments_are_not(self):
        sets = tn.ci_sets(CI)
        lines = [" ".join(kept + (["--"] + harness if harness else [])) for _, kept, harness in sets]
        self.assertEqual(lines, [
            "--workspace",
            "-p a -p b --features x,y --test t1 --test t2",
            "--manifest-path tools/bt/Cargo.toml --locked",
            "-p paperpaw-text --lib linebreak",
            "-p core --test native_plugin",
            "--release -p x4 --test update_path",
            "-p app --test partial_redraw -- --exact table_of_contents",
        ])

    def test_flags_that_only_change_how_it_runs_are_dropped(self):
        kept, harness = tn.normalize("-j 4 --no-fail-fast -q -p a -- --ignored --nocapture".split())
        self.assertEqual((kept, harness), (["-p", "a"], []))

    def test_a_filter_after_the_dashes_is_kept(self):
        self.assertEqual(tn.normalize("-p a -- --exact name".split())[1], ["--exact", "name"])

    def test_ids_are_stable_and_differ_with_the_line(self):
        a = tn.ci_sets("run: cargo test -p a --features x\n")[0][0]
        b = tn.ci_sets("run: cargo test -p a --features y\n")[0][0]
        self.assertEqual(a, tn.ci_sets("run: cargo test -p a --features x\n")[0][0])
        self.assertNotEqual(a, b)

    def test_the_sdk_with_kernel_on_is_a_set_though_ci_has_no_line_for_it(self):
        self.assertTrue(any(s[1] == ["-p", "paperpaw-sdk", "--features", "kernel"] for s in tn.all_sets(CI)))


class Listing(unittest.TestCase):
    def test_binaries_have_no_hash_and_doc_tests_no_line_number(self):
        exe = {"target/debug/deps/flows-0123456789abcdef": "paperpaw-app/flows"}
        text = "\n".join([
            "     Running tests/flows.rs (target/debug/deps/flows-0123456789abcdef)",
            "tests::a: test",
            "b: test",
            "2 tests, 0 benchmarks",
            "   Doc-tests paperpaw_json",
            "crates/paperpaw-json/src/stream.rs - stream (line 12): test",
        ])
        self.assertEqual(tn.parse_listing(text, exe), [
            ("paperpaw-app/flows", "tests::a"),
            ("paperpaw-app/flows", "b"),
            ("paperpaw-json/doc", "crates/paperpaw-json/src/stream.rs - stream"),
        ])

    def test_two_packages_with_a_test_of_one_name_stay_apart(self):
        exe = {"t/x/hosted_construction-aaaaaaaaaaaaaaaa": "p-one/hosted_construction",
               "t/x/hosted_construction-bbbbbbbbbbbbbbbb": "p-two/hosted_construction"}
        text = "Running tests/h.rs (t/x/hosted_construction-aaaaaaaaaaaaaaaa)\nt: test\nRunning tests/h.rs (t/x/hosted_construction-bbbbbbbbbbbbbbbb)\nt: test\n"
        self.assertEqual([b for b, _ in tn.parse_listing(text, exe)], ["p-one/hosted_construction", "p-two/hosted_construction"])

    def test_the_map_comes_from_cargos_json(self):
        message = json.dumps({
            "reason": "compiler-artifact",
            "package_id": "path+file:///w/plugins/home#paperpaw-plugin-home@0.6.0",
            "target": {"kind": ["test"], "name": "flows"},
            "profile": {"test": True},
            "executable": "/w/target/debug/deps/flows-0123456789abcdef",
        })
        lib = json.dumps({
            "reason": "compiler-artifact",
            "package_id": "path+file:///w/crates/paperpaw-core#0.6.0",
            "target": {"kind": ["lib"], "name": "paperpaw_core"},
            "profile": {"test": True},
            "executable": "/w/target/debug/deps/paperpaw_core-0123456789abcdef",
        })
        got = tn.exe_map_of([message, lib, "not json"])
        self.assertEqual(sorted(got.values()), ["paperpaw-core/lib", "paperpaw-plugin-home/flows"])

    def test_lines_mark_ignored_tests(self):
        lines = tn.lines_of([("b", "t1"), ("b", "t2")], [("b", "t2")])
        self.assertEqual(lines, ["b\tt1\trun", "b\tt2\tignored"])


class Comparing(unittest.TestCase):
    old = {("b", "t1"): "run", ("b", "t2"): "run", ("b", "t3"): "ignored"}

    def test_a_missing_name_fails_unless_it_is_listed_with_a_reason(self):
        new = {("b", "t1"): "run", ("b", "t3"): "ignored"}
        added, failures, _ = tn.compare(self.old, new, {})
        self.assertEqual((added, failures), ([], ["missing: b\tt2"]))
        self.assertEqual(tn.compare(self.old, new, {("b", "t2"): "moved to c"})[1], [])

    def test_a_test_that_is_ignored_now_fails_like_a_missing_one(self):
        new = {("b", "t1"): "run", ("b", "t2"): "ignored", ("b", "t3"): "ignored"}
        self.assertEqual(tn.compare(self.old, new, {})[1], ["now ignored: b\tt2"])

    def test_an_added_name_is_reported_not_failed(self):
        new = {**self.old, ("b", "t4"): "run"}
        added, failures, _ = tn.compare(self.old, new, {})
        self.assertEqual((added, failures), ([("b", "t4")], []))

    def test_a_removed_line_needs_a_reason(self):
        self.assertEqual(tn.parse_removed("# c\nb\tt\tmoved\nb\tu\t\n"), {("b", "t"): "moved"})


class Static(unittest.TestCase):
    def test_the_firmware_crate_is_scanned_for_tests(self):
        lines = tn.static_lines("shells/xteink")
        self.assertTrue(lines, "shells/xteink has #[test] functions")
        self.assertTrue(all(line.split("\t")[0] == "shells/xteink/static" for line in lines))


if __name__ == "__main__":
    unittest.main()
