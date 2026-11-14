"""The size gate (tools/size-report.py): it passes within budget and fails loudly otherwise."""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "size-report.py")
REAL_BUDGET = os.path.join(HERE, "..", "shells", "xteink", "size-budget.txt")

spec = importlib.util.spec_from_file_location("size_report", TOOL)
size_report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(size_report)


class SizeGate(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)

    def file(self, name, content):
        path = os.path.join(self.dir.name, name)
        with open(path, "wb") as f:
            f.write(content)
        return path

    def budget(self, text):
        return self.file("budget.txt", text.encode())

    def run_tool(self, device, image, budget):
        done = subprocess.run(
            [sys.executable, "-I", TOOL, "check", "--device", device, "--image", image, "--budget", budget],
            capture_output=True,
            text=True,
        )
        return done.returncode, done.stdout

    def test_an_image_within_its_budget_passes(self):
        image = self.file("a.bin", b"x" * 100)
        code, out = self.run_tool("x4", image, self.budget("image x4 100\n"))
        self.assertEqual(code, 0, out)
        self.assertIn("room 0", out)

    def test_an_image_over_its_budget_fails_with_exit_1(self):
        image = self.file("a.bin", b"x" * 101)
        code, out = self.run_tool("x4", image, self.budget("image x4 100\n"))
        self.assertEqual(code, 1, out)
        self.assertIn("1 bytes over", out)

    def test_each_device_has_its_own_line(self):
        image = self.file("a.bin", b"x" * 150)
        budget = self.budget("image x4 100\nimage x4-pro 200\n")
        self.assertEqual(self.run_tool("x4", image, budget)[0], 1)
        self.assertEqual(self.run_tool("x4-pro", image, budget)[0], 0)

    def test_a_missing_image_fails_with_exit_2(self):
        code, out = self.run_tool("x4", os.path.join(self.dir.name, "none.bin"), self.budget("image x4 100\n"))
        self.assertEqual(code, 2, out)
        self.assertIn("does not exist", out)

    def test_an_empty_image_fails_with_exit_2(self):
        code, _ = self.run_tool("x4", self.file("a.bin", b""), self.budget("image x4 100\n"))
        self.assertEqual(code, 2)

    def test_a_missing_budget_file_fails_with_exit_2(self):
        image = self.file("a.bin", b"x")
        code, out = self.run_tool("x4", image, os.path.join(self.dir.name, "none.txt"))
        self.assertEqual(code, 2, out)
        self.assertIn("cannot be read", out)

    def test_a_device_without_a_line_fails_with_exit_2(self):
        image = self.file("a.bin", b"x")
        code, out = self.run_tool("x3", image, self.budget("image x4 100\n"))
        self.assertEqual(code, 2, out)
        self.assertIn("no `image x3` line", out)

    def test_a_line_that_is_not_understood_is_an_error(self):
        image = self.file("a.bin", b"x")
        for text in (
            "imagee x4 100\n",
            "image x4\n",
            "image x4 10k\n",
            "image x5 100\n",
            "image x4 100\nimage x4 200\n",
            "layer x4 core\n",
        ):
            code, out = self.run_tool("x4", image, self.budget(text))
            self.assertEqual(code, 2, f"{text!r}: {out}")

    def test_comments_blank_lines_and_layer_lines_are_allowed(self):
        image = self.file("a.bin", b"x" * 10)
        text = "# a comment\n\nimage x4 100 # trailing\nlayer x4 core 50\n"
        self.assertEqual(self.run_tool("x4", image, self.budget(text))[0], 0)

    def test_the_committed_budget_names_every_device(self):
        images = size_report.read_budget(REAL_BUDGET)
        self.assertEqual(sorted(images), sorted(size_report.DEVICES))
        # Beta 4's target is 6,400,000 bytes at most (#258). Decision 184 let the
        # first native images (train t40) exceed it until they are shrunk: these
        # ceilings are that train's lines, and only go down (back to 6,400,000).
        ceilings = {
            "x4": 6_493_952,
            "x3": 6_505_744,
            "x4-pro": 7_062_560,
            "x4-classic": 6_612_704,
        }
        for device, budget in images.items():
            self.assertLessEqual(budget, ceilings[device], device)


if __name__ == "__main__":
    unittest.main()
