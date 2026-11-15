"""tools/stack-frames.py: a frame built with `slli` is read whole, and a frame that is not a multiple of 16 is refused."""

import importlib.util
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = os.path.join(HERE, "stack-frames.py")
spec = importlib.util.spec_from_file_location("stack_frames", TOOL)
tool = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tool)

# What objdump prints for a RISC-V prologue that builds 29 << 8 = 7,424 bytes.
SLLI = """\
42000000 <big>:
42000000:	addi	sp,sp,-16
42000004:	li	a0,0x1d
42000008:	slli	a0,a0,8
4200000c:	sub	sp,sp,a0
42000010:	ret
"""


class Slli(unittest.TestCase):
    def test_a_frame_built_with_slli_is_read_whole(self):
        funcs = tool.parse(SLLI.splitlines(), tool.EM_RISCV, None)
        self.assertEqual([f.frame for f in funcs], [16 + 7424])

    def test_the_compressed_shift_is_read_too(self):
        text = SLLI.replace("slli\ta0,a0,8", "c.slli\ta0,8")
        funcs = tool.parse(text.splitlines(), tool.EM_RISCV, None)
        self.assertEqual([f.frame for f in funcs], [16 + 7424])


class Refused(unittest.TestCase):
    def test_a_frame_that_is_not_a_multiple_of_16_is_refused(self):
        text = SLLI.replace("li\ta0,0x1d", "li\ta0,0x1c").replace("slli\ta0,a0,8", "addi\ta0,a0,1")
        funcs = tool.parse(text.splitlines(), tool.EM_RISCV, None)
        self.assertEqual([f.frame for f in funcs], [16 + 29])
        self.assertEqual(tool.odd_frames(funcs), funcs)

    def test_a_whole_frame_and_a_local_label_pass(self):
        funcs = tool.parse(SLLI.splitlines(), tool.EM_RISCV, None)
        label = tool.Function(".Lpcrel_hi5", 0)
        label.frame = 4
        self.assertEqual(tool.odd_frames(funcs + [label]), [])


if __name__ == "__main__":
    unittest.main()
