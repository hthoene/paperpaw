"""Check named batch symbols in a desktop executable or target rlib."""
import re
import subprocess
import sys

for path in sys.argv[1:]:
    sections = subprocess.check_output(["readelf", "-SW", path], text=True)
    symbols = subprocess.check_output(["readelf", "-sW", path], text=True)
    # rlib metadata has no batch symbols; all batches belong to the sample's
    # single data object. Section indices from that last object supersede them.
    names = {}
    for line in sections.splitlines():
        match = re.match(r"\s*\[\s*(\d+)\]\s+(\S+)\s+PROGBITS\s+\S+\s+\S+\s+\S+\s+\S+\s+(\S+)", line)
        if match:
            names[match[1]] = (match[2], match[3])
    batches = []
    for line in symbols.splitlines():
        fields = line.split()
        if len(fields) == 8 and fields[3] == "OBJECT" and "BATCH_" in fields[7]:
            section, flags = names[fields[6]]
            assert section.startswith(".rodata"), (fields[7], section)
            assert "W" not in flags, (fields[7], flags)
            batches.append(int(fields[2]))
    assert len(batches) == 51, (path, len(batches))
    assert sum(batches) == 61815, (path, sum(batches))
    print(f"{path}: 51 batch symbols, 61815 bytes, read-only .rodata")
