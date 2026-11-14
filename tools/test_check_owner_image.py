"""tools/check-owner-image.py: a full image passes; an app-only or changed-table image does not."""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "check-owner-image.py")
spec = importlib.util.spec_from_file_location("check_owner_image", TOOL)
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)


def full_image(table=b"\xaa\x50" + b"T" * 30, boot=b"\xe9" + b"B" * 15, app=b"\xe9" + b"A" * 300):
    img = bytearray(b"\xff" * 0x10200)
    img[0:len(boot)] = boot
    img[0x8000:0x8000 + len(table)] = table
    img[0x10000:0x10000 + len(app)] = app
    return bytes(img)


class OwnerImage(unittest.TestCase):
    def test_a_full_image_with_the_references_table_passes(self):
        self.assertEqual(tool.problems(full_image(), full_image()), [])

    def test_a_different_bootloader_passes_but_is_counted(self):
        other = full_image(boot=b"\xe9" + b"X" * 15)
        self.assertEqual(tool.problems(other, full_image()), [])
        self.assertEqual(tool.bootloader_diff(other, full_image()), 15)

    def test_an_app_only_image_is_refused(self):
        app_only = b"\xe9" + b"A" * 6_000_000
        bad = tool.problems(app_only, full_image())
        self.assertTrue(any("partition table magic" in b for b in bad), bad)
        self.assertTrue(any("app image magic" in b for b in bad), bad)

    def test_a_changed_partition_table_is_refused(self):
        other = full_image(table=b"\xaa\x50" + b"U" * 30)
        self.assertEqual(tool.problems(other, full_image()), ["partition table differs from the reference"])

    def test_missing_magics_and_a_short_file_are_refused_without_a_crash(self):
        for img in (b"", b"\xe9", full_image(boot=b"\x00"), full_image(app=b"\x00" * 8)):
            self.assertTrue(tool.problems(img, full_image()), img[:4])

    def test_a_reference_that_is_not_a_full_image_is_refused(self):
        self.assertIn("the reference is not a full image", tool.problems(full_image(), b"\xe9" * 100))

    def run_tool(self, img, ref):
        with tempfile.TemporaryDirectory() as d:
            paths = []
            for name, data in (("img", img), ("ref", ref)):
                p = os.path.join(d, name)
                with open(p, "wb") as f:
                    f.write(data)
                paths.append(p)
            return subprocess.run([sys.executable, "-I", TOOL, *paths], capture_output=True, text=True)

    def test_the_command_says_its_exit_and_returns_it(self):
        ok = self.run_tool(full_image(), full_image())
        self.assertEqual((ok.returncode, "OWNER_IMAGE_CHECK_EXIT=0" in ok.stdout), (0, True), ok.stdout)
        no = self.run_tool(b"\xe9" + b"A" * 6_000_000, full_image())
        self.assertEqual((no.returncode, "OWNER_IMAGE_CHECK_EXIT=1" in no.stdout), (1, True), no.stdout)
        usage = subprocess.run([sys.executable, "-I", TOOL], capture_output=True, text=True)
        self.assertEqual(usage.returncode, 2)


if __name__ == "__main__":
    unittest.main()
