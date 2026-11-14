"""Tests of check_files.sh: what store-plugin.yml's signing job accepts of a build's files.

    python3 -m unittest discover -s tools/plugin-files
"""

import pathlib
import subprocess
import tempfile
import unittest

SCRIPT = pathlib.Path(__file__).resolve().parent / "check_files.sh"
# A stand-in for `paperpaw-sign plugin <file>`, which prints what the file says.
SIGN = '#!/bin/sh\nprintf "chip:        ESP32-C3 (5)\\nfingerprint: 00053e32940609b9\\n"\n'


class CheckFiles(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = pathlib.Path(self.tmp.name)
        self.dir = root / "files"
        self.dir.mkdir()
        self.sign = root / "sign"
        self.sign.write_text(SIGN)
        self.sign.chmod(0o755)
        (self.dir / "x-1.0.0-x4.ppn").write_bytes(b"file")

    def run_check(self, chip="5", fingerprint="00053e32940609b9", name="x-1.0.0-x4.ppn"):
        return subprocess.run([str(SCRIPT), str(self.dir), str(self.sign), chip, fingerprint, name],
                              capture_output=True, text=True).returncode

    def test_the_expected_file_passes(self):
        self.assertEqual(self.run_check(), 0)

    def test_another_chip_fingerprint_or_name_fails(self):
        self.assertEqual(self.run_check(chip="9"), 1)
        self.assertEqual(self.run_check(fingerprint="0005933c6279fe30"), 1)
        self.assertEqual(self.run_check(fingerprint=""), 1)
        self.assertEqual(self.run_check(name="y-1.0.0-x4.ppn"), 1)

    def test_a_file_of_interface_7_has_no_fingerprint_but_says_its_format(self):
        sign5 = self.sign.parent / "sign5"
        sign5.write_text('#!/bin/sh\nprintf "format:      5 (interface 7)\\nchip:        ESP32-C3 (5)\\n"\n')
        sign5.chmod(0o755)
        def check(sign, chip="5"):
            return subprocess.run([str(SCRIPT), str(self.dir), str(sign), chip, "none", "x-1.0.0-x4.ppn"],
                                  capture_output=True, text=True).returncode
        self.assertEqual(check(sign5), 0)
        self.assertEqual(check(sign5, chip="9"), 1)
        # A file of the old format, or a tool that says no format: refused.
        self.assertEqual(check(self.sign), 1)

    def test_a_second_file_or_a_link_fails(self):
        (self.dir / "extra.ppn").write_bytes(b"more")
        self.assertEqual(self.run_check(), 1)
        (self.dir / "extra.ppn").unlink()
        (self.dir / "x-1.0.0-x4.ppn").unlink()
        (self.dir / "x-1.0.0-x4.ppn").symlink_to("/etc/passwd")
        self.assertEqual(self.run_check(), 1)


if __name__ == "__main__":
    unittest.main()
