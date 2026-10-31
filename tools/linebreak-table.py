"""Writes the line-break class table of paperpaw-text (src/linebreak) from the
Unicode Character Database (UCD).

Usage:
    python3 tools/linebreak-table.py <ucd-dir>            write the table
    python3 tools/linebreak-table.py <ucd-dir> --check    fail if the table in the tree differs

<ucd-dir> holds, from https://www.unicode.org/Public/17.0.0/ucd/:
LineBreak.txt, EastAsianWidth.txt, UnicodeData.txt, Scripts.txt, Blocks.txt,
emoji/emoji-data.txt (as emoji-data.txt) and auxiliary/LineBreakTest.txt (as
LineBreakTest.txt; only the tests read it). Every file is checked against the
SHA-256 sum below: a changed mirror cannot change the table silently. The UCD
is under the Unicode License v3 (see NOTICE).

The table (design: paperpaw-notes designs/line-breaking-a5.md, section 4):
the line-break class of every code point, resolved as far as the rules do not
need the original (LB1: AI, SG, XX -> AL; SA -> CM for Mn and Mc, else AL;
Pi and Pf quotation marks split off QU; unassigned Extended_Pictographic as
IDEP or ALEP: their class and the mark LB30b needs), with two flags, the East Asian width (F, W, H) and "UAX #14 decides
here" (the compat profile). The Hangul syllables U+AC00..U+D7A3 are not
stored: H2 for every 28th, H3 for the others (checked below against the data).

Format 1 (`PPLB`):
    header      "PPLB", format u8, unicode major u8, minor u8, update u8,
                class count u8, reserved u8, index len u16, stream len u16
    ascii       128 bytes: class | east asian << 6 | uax << 7
    index       index len x { start u32, offset u16 }, one per 32 runs
    stream      per run: that byte, then the run's length in code points as
                a LEB128 varint; the runs cover U+0080..U+10FFFF
"""

import hashlib
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates/paperpaw-text/src/linebreak/table_data.rs"

SUMS = {
    "LineBreak.txt": "e6a18fa91f8f6a6f8e534b1d3f128c21ada45bfe152eb6b1bcc5e15fd8ac92e6",
    "LineBreakTest.txt": "e69884e0dde6a8724873f885d68c52dc14518abf9ae4ca9e2283b8773db3b752",
    "EastAsianWidth.txt": "ea7ce50f3444a050333448dffef1cadd9325af55cbb764b4a2280faf52170a33",
    "emoji-data.txt": "2cb2bb9455cda83e8481541ecf5b6dfda66a3bb89efa3fa7c5297eccf607b72b",
    "UnicodeData.txt": "2e1efc1dcb59c575eedf5ccae60f95229f706ee6d031835247d843c11d96470c",
    "Scripts.txt": "9f5e50d3abaee7d6ce09480f325c706f485ae3240912527e651954d2d6b035bf",
    "Blocks.txt": "c0edefaf1a19771e830a82735472716af6bf3c3975f6c2a23ffbe2580fbbcb15",
}
VERSION = (17, 0, 0)

# The ids are append-only: a new Unicode class takes the next one. The order
# is the one of `Class` in table_data.rs, which this script writes.
CLASSES = [
    "AL", "AK", "AP", "AS", "B2", "BA", "BB", "BK", "CB", "CJ", "CL", "CM",
    "CP", "CR", "EB", "EM", "EX", "GL", "H2", "H3", "HH", "HL", "HY", "ID",
    "IDEP", "IN", "IS", "JL", "JT", "JV", "LF", "NL", "NS", "NU", "OP", "PO",
    "PR", "QU", "QUPI", "QUPF", "RI", "SP", "SY", "VF", "VI", "WJ", "ZW", "ZWJ",
    "ALEP",
]
# Where UAX #14 decides (the compat profile): classes, scripts and blocks.
UAX_CLASSES = {"ID", "IDEP", "ALEP", "CJ", "H2", "H3", "JL", "JV", "JT", "ZW", "WJ"}
UAX_SCRIPTS = {
    "Han", "Hiragana", "Katakana", "Hangul", "Bopomofo", "Yi", "Thai", "Lao",
    "Khmer", "Myanmar", "Tibetan", "Ethiopic", "Mongolian",
}
UAX_BLOCKS = {
    "CJK Symbols and Punctuation", "Halfwidth and Fullwidth Forms",
    "Vertical Forms", "Small Form Variants", "CJK Compatibility Forms",
    "Hangul Jamo Extended-A", "Hangul Jamo Extended-B",
    "Hangul Compatibility Jamo", "Kana Supplement", "Kana Extended-A",
    "Kana Extended-B", "Small Kana Extension", "Yi Radicals",
    "Bopomofo Extended",
}
HANGUL = (0xAC00, 0xD7A3)
LAST = 0x10FFFF
BLOCK = 32  # runs per index entry


def rows(path):
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            yield [field.strip() for field in line.split(";")]


def span(text):
    first, _, last = text.partition("..")
    return int(first, 16), int(last or first, 16)


def check_inputs(ucd):
    for name, want in SUMS.items():
        path = ucd / name
        if not path.is_file():
            sys.exit(f"{path}: missing (see the usage in {Path(__file__).name})")
        got = hashlib.sha256(path.read_bytes()).hexdigest()
        if got != want:
            sys.exit(f"{path}: SHA-256 {got}, expected {want} (not the Unicode {'.'.join(map(str, VERSION))} file)")
    first = (ucd / "LineBreak.txt").read_text(encoding="utf-8").splitlines()[0]
    found = re.search(r"-(\d+)\.(\d+)\.(\d+)\.txt", first)
    if not found or tuple(map(int, found.groups())) != VERSION:
        sys.exit(f"LineBreak.txt: first line {first!r} is not Unicode {VERSION}")


def build(ucd):
    check_inputs(ucd)
    lb = ["XX"] * (LAST + 1)
    for fields in rows(ucd / "LineBreak.txt"):
        first, last = span(fields[0])
        for cp in range(first, last + 1):
            lb[cp] = fields[1]
    gc = {}
    prev = None
    for fields in rows(ucd / "UnicodeData.txt"):
        cp, name = int(fields[0], 16), fields[1]
        if name.endswith(", First>"):
            prev = cp
        elif name.endswith(", Last>"):
            for c in range(prev, cp + 1):
                gc[c] = fields[2]
        else:
            gc[cp] = fields[2]
    wide = [False] * (LAST + 1)
    for fields in rows(ucd / "EastAsianWidth.txt"):
        first, last = span(fields[0])
        if fields[1] in ("F", "W", "H"):
            for cp in range(first, last + 1):
                wide[cp] = True
    pict = set()
    for fields in rows(ucd / "emoji-data.txt"):
        if fields[1] == "Extended_Pictographic":
            first, last = span(fields[0])
            pict.update(range(first, last + 1))
    script = {}
    for fields in rows(ucd / "Scripts.txt"):
        first, last = span(fields[0])
        for cp in range(first, last + 1):
            script[cp] = fields[1]
    block = [None] * (LAST + 1)
    for fields in rows(ucd / "Blocks.txt"):
        first, last = span(fields[0])
        for cp in range(first, last + 1):
            block[cp] = fields[1]

    entries = []
    for cp in range(LAST + 1):
        c = lb[cp]
        if c in ("AI", "SG", "XX"):
            c = "AL"
        elif c == "SA":
            c = "CM" if gc.get(cp) in ("Mn", "Mc") else "AL"
        elif c == "QU":
            c = {"Pi": "QUPI", "Pf": "QUPF"}.get(gc.get(cp), "QU")
        if cp in pict and gc.get(cp, "Cn") == "Cn":
            # An unassigned pictograph keeps its class (ID in the reserved
            # emoji planes, else AL) and the mark LB30b asks for.
            c = "IDEP" if c == "ID" else "ALEP"
        uax = (
            c in UAX_CLASSES
            or script.get(cp) in UAX_SCRIPTS
            or block[cp] in UAX_BLOCKS
        )
        entries.append(CLASSES.index(c) | (64 if wide[cp] else 0) | (128 if uax else 0))
    # The Hangul syllables are computed, not stored: H2 for every 28th.
    for cp in range(HANGUL[0], HANGUL[1] + 1):
        want = "H2" if (cp - HANGUL[0]) % 28 == 0 else "H3"
        if lb[cp] != want:
            sys.exit(f"U+{cp:04X}: LineBreak.txt says {lb[cp]}, the rule says {want}")
        if not wide[cp] or not (entries[cp] & 128):
            sys.exit(f"U+{cp:04X}: a Hangul syllable must be wide and flagged")
    # Inside the hole the entry of the code point before it, so that no run
    # starts there.
    for cp in range(HANGUL[0], HANGUL[1] + 1):
        entries[cp] = entries[HANGUL[0] - 1]
    return entries


def varint(n):
    out = bytearray()
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            out.append(byte | 0x80)
        else:
            out.append(byte)
            return out


def encode(entries):
    ascii_part = bytes(entries[:128])
    runs = []  # (start, length, byte)
    start = 0x80
    for cp in range(0x80, LAST + 2):
        if cp == LAST + 1 or entries[cp] != entries[start]:
            runs.append((start, cp - start, entries[start]))
            start = cp
    stream = bytearray()
    index = []
    for n, (start, length, byte) in enumerate(runs):
        if n % BLOCK == 0:
            index.append((start, len(stream)))
        stream.append(byte)
        stream += varint(length)
    assert len(stream) < 65536 and len(index) < 65536
    head = (
        b"PPLB"
        + bytes([1, *VERSION, len(CLASSES), 0])
        + len(index).to_bytes(2, "little")
        + len(stream).to_bytes(2, "little")
    )
    body = bytearray(head) + ascii_part
    for start, offset in index:
        body += start.to_bytes(4, "little") + offset.to_bytes(2, "little")
    body += stream
    return bytes(body), len(runs)


def render(data, runs):
    lines = [
        "// Generated by tools/linebreak-table.py from the Unicode Character",
        f"// Database {'.'.join(map(str, VERSION))} (Unicode License v3, see NOTICE). Do not edit.",
        "",
        "#![allow(clippy::upper_case_acronyms, reason = \"the names of UAX #14\")]",
        "",
        "/// The line-break classes of UAX #14 as the table keeps them (see the",
        "/// generator). The ids are append-only.",
        "#[derive(Clone, Copy, PartialEq, Eq, Debug)]",
        "#[repr(u8)]",
        "pub(crate) enum Class {",
    ]
    lines += [f"    {name} = {i}," for i, name in enumerate(CLASSES)]
    lines += [
        "}",
        "",
        "/// The classes by id, for decoding.",
        "pub(super) const CLASSES: [Class; %d] = [" % len(CLASSES),
    ]
    per = 8
    names = [f"Class::{n}" for n in CLASSES]
    for i in range(0, len(names), per):
        lines.append("    " + ", ".join(names[i : i + per]) + ",")
    lines += [
        "];",
        "",
        f"/// The Unicode version of the table.",
        f"pub(super) const UNICODE: (u8, u8, u8) = {VERSION};",
        f"/// Runs in the stream (for the tests).",
        f"pub(super) const RUNS: usize = {runs};",
        "",
        f"pub(super) static TABLE: [u8; {len(data)}] = [",
    ]
    for i in range(0, len(data), 16):
        lines.append("    " + ", ".join(f"0x{b:02x}" for b in data[i : i + 16]) + ",")
    lines.append("];")
    return "\n".join(lines) + "\n"


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 1:
        sys.exit(__doc__)
    ucd = Path(args[0])
    data, runs = encode(build(ucd))
    text = render(data, runs)
    if "--check" in sys.argv:
        if not OUT.is_file() or OUT.read_text(encoding="utf-8") != text:
            sys.exit(f"{OUT.relative_to(ROOT)} is not what this script makes from {ucd}: run it without --check")
        print("linebreak table is current")
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(text, encoding="utf-8")
    print(f"{OUT.relative_to(ROOT)}: {runs} runs, {len(data)} bytes of table")


if __name__ == "__main__":
    main()
