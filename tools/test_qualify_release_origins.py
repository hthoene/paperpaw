"""Checksum evidence must fail closed before release bytes enter host tests."""

import hashlib
import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("origins", Path(__file__).with_name("qualify-release-origins.py"))
origins = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(origins)


class Checksums(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.path = self.root / "firmware.bin"
        self.path.write_bytes(b"release evidence")
        self.digest = hashlib.sha256(self.path.read_bytes()).hexdigest()
        self.checksum = self.root / "firmware.bin.sha256"

    def test_exact_text_and_binary_records(self):
        for marker in ("", "*"):
            self.checksum.write_text(f"{self.digest}  {marker}firmware.bin\n")
            self.assertEqual(origins.verified(self.path), self.digest)

    def test_changed_file_fails(self):
        self.checksum.write_text(f"{self.digest}  firmware.bin\n")
        self.path.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            origins.verified(self.path)

    def test_wrong_filename_is_not_accepted(self):
        for name in ("another.bin", "../firmware.bin", "$(touch-x)"):
            self.checksum.write_text(f"{self.digest}  {name}\n")
            with self.assertRaisesRegex(ValueError, "another file"):
                origins.verified(self.path)

    def test_malformed_or_multiple_records_fail(self):
        for record in ("", "not-a-hash firmware.bin", self.digest,
                       f"{self.digest} firmware.bin\n{self.digest} other.bin"):
            self.checksum.write_text(record)
            with self.assertRaisesRegex(ValueError, "one SHA-256"):
                origins.verified(self.path)

    def test_missing_checksum_is_not_a_pass(self):
        with self.assertRaises(FileNotFoundError):
            origins.verified(self.path)

class Evidence(unittest.TestCase):
    def setUp(self):
        from types import SimpleNamespace
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.cache = self.root / "cache"
        self.helper = self.root / "sign"
        self.helper.write_bytes(b"verifier")
        for device in origins.DEVICES:
            directory = self.cache / origins.TAGS[0]
            directory.mkdir(parents=True, exist_ok=True)
            prefix = f"paperpaw-{device}-{origins.TAGS[0][1:]}"
            for ending in (".bin", "-install.bin", "-update.bin"):
                path = directory / (prefix + ending)
                path.write_bytes((device + ending).encode())
                path.with_name(path.name + ".sha256").write_text(
                    f"{origins.digest(path)}  {path.name}\n")
        self.args = SimpleNamespace(cache=self.cache, out=self.root / "one.json",
                                    tag=[origins.TAGS[0]], sign_tool=self.helper,
                                    download=False, candidate=[])

    def receipt(self, env):
        manifest = Path(env["PAPERPAW_RELEASE_ORIGINS"])
        lines = [f"manifest-sha256\t{origins.digest(manifest)}"]
        for line in manifest.read_text().splitlines():
            tag, device, *paths = line.split("\t")
            lines.append("\t".join(("checked", tag, device, *(origins.digest(Path(p)) for p in paths))))
        Path(env["PAPERPAW_RELEASE_RECEIPT"]).write_text("\n".join(lines) + "\n")

    def invoke(self, runner, identity=None):
        import contextlib
        import io
        from unittest.mock import patch
        with patch.object(origins, "run", return_value="checked"), \
             patch.object(origins, "source_identity", side_effect=identity or ["a" * 40, "a" * 40]), \
             patch.object(origins.subprocess, "run", side_effect=runner), \
             contextlib.redirect_stdout(io.StringIO()):
            return origins.audit(self.args)

    def test_exit_zero_without_executed_test_receipt_is_failure(self):
        from types import SimpleNamespace
        with self.assertRaisesRegex(ValueError, "executed-test receipt"):
            self.invoke(lambda *args, **kwargs: SimpleNamespace(returncode=0, stdout="0 tests"))
        self.assertFalse(self.args.out.exists())

    def test_executed_rows_and_clean_source_produce_host_only_pass(self):
        from types import SimpleNamespace
        import json
        def runner(*args, **kwargs):
            self.receipt(kwargs["env"])
            return SimpleNamespace(returncode=0, stdout="tested")
        self.assertEqual(self.invoke(runner), 0)
        report = json.loads(self.args.out.read_text())
        self.assertEqual(report["host_result"], "PASS")
        self.assertEqual(report["source"], "a" * 40)
        self.assertIn("INCOMPLETE", report["qualification"])

    def test_source_change_after_execution_cannot_be_pass(self):
        from types import SimpleNamespace
        def runner(*args, **kwargs):
            self.receipt(kwargs["env"])
            return SimpleNamespace(returncode=0, stdout="tested")
        with self.assertRaisesRegex(ValueError, "changed during"):
            self.invoke(runner, ["a" * 40, "b" * 40])

    def test_interleaved_runs_have_separate_manifests_and_receipts(self):
        from types import SimpleNamespace
        import contextlib
        import io
        from unittest.mock import patch
        manifests = []
        nested = False
        def runner(*args, **kwargs):
            nonlocal nested
            manifest = Path(kwargs["env"]["PAPERPAW_RELEASE_ORIGINS"])
            manifests.append(manifest)
            if not nested:
                nested = True
                other = SimpleNamespace(**vars(self.args))
                other.out = self.root / "two.json"
                origins.audit(other)
            self.receipt(kwargs["env"])
            return SimpleNamespace(returncode=0, stdout="tested")
        with patch.object(origins, "run", return_value="checked"), \
             patch.object(origins, "source_identity", return_value="a" * 40), \
             patch.object(origins.subprocess, "run", side_effect=runner), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(origins.audit(self.args), 0)
        self.assertEqual(len(set(manifests)), 2)
        self.assertTrue(all(path.is_file() for path in manifests))

    def test_dirty_checkout_is_refused_before_qualification(self):
        from unittest.mock import patch
        with patch.object(origins, "run", side_effect=["a" * 40, " M file.rs"]):
            with self.assertRaisesRegex(ValueError, "clean"):
                origins.source_identity()


if __name__ == "__main__":
    unittest.main()
