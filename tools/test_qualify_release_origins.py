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


if __name__ == "__main__":
    unittest.main()
