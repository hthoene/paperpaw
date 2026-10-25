"""Keeps the images make.sh made sparse, and puts part.img into a disk
with an MBR (mbr.sparse): `PPSPARSE`, the image's size (8 bytes), then
each block that is not all zeros as its number (4 bytes) and its 512
bytes."""

import struct


def sparse(src, dst):
    data = open(src, "rb").read()
    out = bytearray(b"PPSPARSE" + struct.pack("<Q", len(data)))
    for at in range(0, len(data), 512):
        block = data[at : at + 512]
        if any(block):
            out += struct.pack("<I", at // 512) + block
    open(dst, "wb").write(out)


part = open("part.img", "rb").read()
start = 2048
mbr = bytearray(512)
mbr[446:462] = struct.pack("<B3sB3sII", 0, b"\0\0\0", 0x07, b"\0\0\0", start, len(part) // 512)
mbr[510:512] = b"\x55\xaa"
open("mbr.img", "wb").write(bytes(mbr) + bytes(start * 512 - 512) + part)

for name in ["small", "big32k", "filled", "mbr"]:
    sparse(name + ".img", name + ".sparse")
