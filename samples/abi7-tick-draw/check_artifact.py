"""Checks the release cdylib `verify.sh` built, without loading PaperPaw.

Usage: python3 -I check_artifact.py <library.so>

1. The dynamic symbol table holds exactly `PAPERPAW_ENTRY` and
   `PAPERPAW_METADATA` (no other export, no import), and the library needs no
   other library (`readelf -d` shows no NEEDED).
2. `copy_bytes`, the code behind the hidden `memcpy`, does not call or jump
   to `memcpy`: LLVM may turn a byte-copy loop in a `memcpy` definition back
   into a call to `memcpy`, which then recurses until the stack overflows.
   Neither `nm` nor loading the library shows it.
3. The hidden `memcpy` and `copy_bytes` copy correctly, called in the loaded
   library: sizes around and above 64 bytes, unaligned starts, and the bytes
   around the region left alone. Hidden symbols are not exported, so their
   addresses come from the symbol table, relative to an exported symbol.
"""
import ctypes
import re
import subprocess
import sys

EXPORTS = {"PAPERPAW_ENTRY", "PAPERPAW_METADATA"}


def run(*command):
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout


def symbols(so, *flags):
    table = {}
    for line in run("nm", *flags, so).splitlines():
        parts = line.split()
        if len(parts) == 3:
            table[parts[2]] = int(parts[0], 16)
    return table


def check_dynamic(so):
    defined = set(symbols(so, "-D", "--defined-only"))
    if defined != EXPORTS:
        sys.exit("dynamic exports %s, expected exactly %s" % (sorted(defined), sorted(EXPORTS)))
    undefined = run("nm", "-D", "--undefined-only", so).split()
    if undefined:
        sys.exit("undefined symbols: %s" % undefined)
    needed = [line for line in run("readelf", "-d", so).splitlines() if "(NEEDED)" in line]
    if needed:
        sys.exit("needed libraries:\n" + "\n".join(needed))
    print("dynamic symbols: exactly %s; nothing imported or needed" % sorted(EXPORTS))


def local(so, pattern):
    found = {n: a for n, a in symbols(so).items() if re.search(pattern, n)}
    if len(found) != 1:
        sys.exit("expected one symbol matching %r in the symbol table, found %s" % (pattern, sorted(found)))
    return next(iter(found.items()))


def check_disassembly(so):
    name, _ = local(so, r"copy_bytes")
    text = run("objdump", "-d", "--no-show-raw-insn", "--disassemble=" + name, so)
    body = [line for line in text.splitlines() if re.match(r"\s+[0-9a-f]+:\s", line)]
    if not body:
        sys.exit("copy_bytes has no code in " + so)
    calls = [line for line in body if re.search(r"\b(call|jmp)\w*\s.*<(memcpy|memmove|memset)(@plt)?>", line)]
    if calls:
        sys.exit("copy_bytes calls memcpy, memmove or memset:\n" + "\n".join(calls))
    print("copy_bytes: %d instructions, no call to memcpy" % len(body))


def check_copies(so):
    lib = ctypes.CDLL(so)
    exported = ctypes.addressof(ctypes.c_char.in_dll(lib, "PAPERPAW_METADATA"))
    base = exported - symbols(so, "-D", "--defined-only")["PAPERPAW_METADATA"]
    for pattern in (r"^memcpy$", r"copy_bytes"):
        _, address = local(so, pattern)
        copy = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t)(base + address)
        for size in (0, 1, 7, 8, 63, 64, 65, 127, 200, 4096, 65537):
            for offset in (0, 1, 3):
                pattern_bytes = bytes((i * 31 + size) % 251 for i in range(size))
                source = (ctypes.c_char * (size + 8 + offset))()
                target = (ctypes.c_char * (size + 16))()
                ctypes.memset(target, 0xAA, size + 16)
                src = ctypes.addressof(source) + offset
                dst = ctypes.addressof(target) + 4
                ctypes.memmove(src, pattern_bytes, size)  # the test's own copy, libc's
                result = copy(dst, src, size)
                assert result == dst, "memcpy must return its destination"
                got = bytes(target)
                assert got[4:4 + size] == pattern_bytes, "wrong bytes for %d at offset %d" % (size, offset)
                assert got[:4] == b"\xaa" * 4, "wrote before the region"
                assert got[4 + size:] == b"\xaa" * 12, "wrote after the region (%d)" % size
    print("memcpy and copy_bytes: copies correct for sizes up to 65537")


if len(sys.argv) != 2:
    sys.exit(__doc__)
check_dynamic(sys.argv[1])
check_disassembly(sys.argv[1])
check_copies(sys.argv[1])
