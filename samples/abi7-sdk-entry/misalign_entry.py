"""Make a negative fixture by moving only the ELF32 entry symbol four bytes."""
from pathlib import Path
import struct
import sys

image = bytearray(Path(sys.argv[1]).read_bytes())
assert image[:6] == b"\x7fELF\x01\x01", "expected little-endian ELF32"
section_at = struct.unpack_from("<I", image, 32)[0]
section_bytes, section_count = struct.unpack_from("<HH", image, 46)
for index in range(section_count):
    section = section_at + index * section_bytes
    if struct.unpack_from("<I", image, section + 4)[0] != 2:  # SHT_SYMTAB
        continue
    start, length, strings = struct.unpack_from("<III", image, section + 16)
    symbol_bytes = struct.unpack_from("<I", image, section + 36)[0]
    string_at = struct.unpack_from("<I", image, section_at + strings * section_bytes + 16)[0]
    for symbol in range(start, start + length, symbol_bytes):
        name = string_at + struct.unpack_from("<I", image, symbol)[0]
        name_end = image.index(0, name)
        if image[name:name_end] == b"PAPERPAW_ENTRY":
            address = struct.unpack_from("<I", image, symbol + 4)[0]
            assert address % 8 == 0, "positive fixture must start aligned"
            struct.pack_into("<I", image, symbol + 4, address + 4)
            Path(sys.argv[2]).write_bytes(image)
            raise SystemExit(0)
raise SystemExit("no PAPERPAW_ENTRY symbol in linked sample")
