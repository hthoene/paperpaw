"""Checks the release cdylib `verify.sh` built, without loading PaperPaw.

Usage: python3 -I check_artifact.py <library.so>

1. `memcpy` does not call or jump to `memcpy`: LLVM may turn a byte-copy loop
   in a `memcpy` definition back into a call to `memcpy`, which then recurses
   until the stack overflows. Neither `nm` nor loading the library shows it.
2. `memcpy` copies correctly through the library: sizes around and above 64
   bytes, unaligned starts, and bytes around the region left alone.
"""
import ctypes
import re
import subprocess
import sys


def check_disassembly(so):
    text = subprocess.run(
        ["objdump", "-d", "--no-show-raw-insn", "--disassemble=memcpy", so],
        check=True, capture_output=True, text=True,
    ).stdout
    body = [line for line in text.splitlines() if re.match(r"\s+[0-9a-f]+:\s", line)]
    if not body:
        sys.exit("memcpy has no code in " + so)
    calls = [line for line in body if re.search(r"\b(call|jmp)\w*\s.*<memcpy(@plt)?>", line)]
    if calls:
        sys.exit("memcpy calls itself:\n" + "\n".join(calls))
    print("memcpy: %d instructions, no call to memcpy" % len(body))


def check_copies(so):
    lib = ctypes.CDLL(so)
    lib.memcpy.restype = ctypes.c_void_p
    lib.memcpy.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
    for size in (0, 1, 7, 8, 63, 64, 65, 127, 200, 4096, 65537):
        for offset in (0, 1, 3):
            pattern = bytes((i * 31 + size) % 251 for i in range(size))
            source = (ctypes.c_char * (size + 8 + offset))()
            target = (ctypes.c_char * (size + 16))()
            ctypes.memset(target, 0xAA, size + 16)
            src = ctypes.addressof(source) + offset
            dst = ctypes.addressof(target) + 4
            ctypes.memmove(src, pattern, size)  # the test's own copy, libc's
            result = lib.memcpy(dst, src, size)
            assert result == dst, "memcpy must return its destination"
            assert bytes(target)[4:4 + size] == pattern, "wrong bytes for %d at offset %d" % (size, offset)
            assert bytes(target)[:4] == b"\xaa" * 4, "wrote before the region"
            assert bytes(target)[4 + size:] == b"\xaa" * (12), "wrote after the region (%d)" % size
    print("memcpy: copies correct for sizes up to 65537")


if len(sys.argv) != 2:
    sys.exit(__doc__)
check_disassembly(sys.argv[1])
check_copies(sys.argv[1])
