"""tools/check-s3-release-exceptions.py: only `main` may stay an `allow frame` line at a release."""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "check-s3-release-exceptions.py")
spec = importlib.util.spec_from_file_location("check_s3_release_exceptions", TOOL)
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)

MAIN = "allow frame main 8768 the firmware's entry on the main task's stack; owner: Firmware\n"
OTHER = "allow frame paperpaw_core::chrome::sync_chromes 2256 owner: Kernel todo 626; todo 639\n"
NOISE = "# a comment\nroot paperpaw_sdk::native::binding::entry\nallow indirect paperpaw_sdk 404 why\nframes-in paperpaw_sdk\n"


class ReleaseExceptions(unittest.TestCase):
    def run_tool(self, text):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "linked.txt")
            with open(path, "w") as f:
                f.write(text)
            return subprocess.run([sys.executable, "-I", TOOL, path], capture_output=True, text=True)

    def test_only_main_left_passes(self):
        self.assertEqual(tool.left(NOISE + MAIN), [])
        done = self.run_tool(NOISE + MAIN)
        self.assertEqual(done.returncode, 0, done.stdout)
        self.assertIn("CHECK_S3_RELEASE_EXCEPTIONS_EXIT=0", done.stdout)

    def test_any_other_exception_fails_and_is_named(self):
        done = self.run_tool(NOISE + OTHER + MAIN)
        self.assertEqual(done.returncode, 1, done.stdout)
        self.assertIn("sync_chromes", done.stdout)
        self.assertIn("CHECK_S3_RELEASE_EXCEPTIONS_EXIT=1", done.stdout)
        self.assertEqual([e[1] for e in tool.left(OTHER + MAIN)], ["paperpaw_core::chrome::sync_chromes"])

    def test_allow_indirect_and_other_allow_kinds_are_not_exceptions(self):
        self.assertEqual(tool.left("allow indirect frame 1 x\nallow rom y\nallow frames z 1\n" + MAIN), [])

    def test_a_missing_file_and_bad_usage_exit_2(self):
        gone = subprocess.run([sys.executable, "-I", TOOL, "/nonexistent/linked.txt"], capture_output=True, text=True)
        self.assertEqual(gone.returncode, 2)
        usage = subprocess.run([sys.executable, "-I", TOOL, "a", "b"], capture_output=True, text=True)
        self.assertEqual(usage.returncode, 2)

    def test_the_real_budget_is_read_when_it_is_there(self):
        real = tool.DEFAULT
        if real.exists():
            self.assertIsInstance(tool.left(real.read_text()), list)


if __name__ == "__main__":
    unittest.main()
