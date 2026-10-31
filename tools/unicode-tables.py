"""Writes the Unicode tables of paperpaw-text's bidi and arabic modules
from the Unicode Character Database (UCD).

Usage:
    python3 tools/unicode-tables.py <ucd-dir>

<ucd-dir> holds, from https://www.unicode.org/Public/<version>/ucd/:
UnicodeData.txt, BidiBrackets.txt, BidiMirroring.txt and, from its
`extracted/` folder, DerivedBidiClass.txt and DerivedJoiningType.txt.
The version is read from the files' first lines and written into the
tables. The UCD is under the Unicode License v3 (see NOTICE).

It writes:
    crates/paperpaw-text/src/bidi/tables.rs
    crates/paperpaw-text/src/arabic/tables.rs
    crates/paperpaw-text/src/mark_table.rs
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BIDI_OUT = ROOT / "crates/paperpaw-text/src/bidi/tables.rs"
ARABIC_OUT = ROOT / "crates/paperpaw-text/src/arabic/tables.rs"
MARKS_OUT = ROOT / "crates/paperpaw-text/src/mark_table.rs"
# A run of marks in `MARKS`: its first code point shifted by this, its
# length less one below it.
MARK_SHIFT = 11

# The order of `bidi::Class`.
CLASSES = ["L", "R", "AL", "EN", "ES", "ET", "AN", "CS", "NSM", "BN", "B", "S", "WS", "ON",
           "LRE", "LRO", "RLE", "RLO", "PDF", "LRI", "RLI", "FSI", "PDI"]
LONG = {"Left_To_Right": "L", "Right_To_Left": "R", "Arabic_Letter": "AL",
        "European_Terminator": "ET"}
# The order of `arabic::Joining`.
JOININGS = ["U", "R", "L", "D", "C", "T"]
# The blocks the arabic module shapes: Arabic, Arabic Supplement,
# Arabic Extended-B and -A.
ARABIC = (0x0600, 0x08FF)
# A form's index in a letter's table: as `arabic::Form`.
FORMS = {"<isolated>": 0, "<final>": 1, "<initial>": 2, "<medial>": 3}
LAM = 0x0644
# The alefs lam takes as a mandatory ligature (rlig).
ALEFS = (0x0622, 0x0623, 0x0625, 0x0627)


def rows(path):
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            yield [field.strip() for field in line.split(";")]


def span(text):
    first, _, last = text.partition("..")
    return int(first, 16), int(last or first, 16)


def version(path):
    first = path.read_text(encoding="utf-8").splitlines()[0]
    found = re.search(r"-(\d+\.\d+\.\d+)\.txt", first)
    if not found:
        sys.exit(f"{path}: no version in its first line")
    return found.group(1)


def bidi_classes(ucd):
    path = ucd / "DerivedBidiClass.txt"
    table = ["L"] * 0x110000
    for line in path.read_text(encoding="utf-8").splitlines():
        found = re.match(r"# @missing: ([0-9A-F.]+); (\w+)", line)
        if found:
            first, last = span(found.group(1))
            table[first:last + 1] = [LONG[found.group(2)]] * (last - first + 1)
    for code, value in rows(path):
        first, last = span(code)
        table[first:last + 1] = [value] * (last - first + 1)
    return table


def runs(table, first, last):
    out, previous = [], None
    for cp in range(first, last + 1):
        if table[cp] != previous:
            out.append((cp, table[cp]))
            previous = table[cp]
    return out


def numbers(values, per_line, width):
    items = [f"{v:#0{width}x}" if isinstance(v, int) and width else str(v) for v in values]
    lines = []
    for i in range(0, len(items), per_line):
        lines.append("    " + ", ".join(items[i:i + per_line]) + ",")
    return "\n".join(lines)


def array(name, kind, values, per_line=12, width=0):
    return (f"pub(super) static {name}: [{kind}; {len(values)}] = [\n"
            f"{numbers(values, per_line, width)}\n];\n")


def bidi_tables(ucd, ver):
    table = bidi_classes(ucd)
    bmp = runs(table, 0, 0xFFFF)
    rest = runs(table, 0x10000, 0x10FFFF)
    brackets = sorted((int(cp, 16), int(pair, 16), kind) for cp, pair, kind in rows(ucd / "BidiBrackets.txt"))
    mirrors = sorted((int(cp, 16), int(m, 16)) for cp, m in rows(ucd / "BidiMirroring.txt"))
    for cp, pair, _ in brackets:
        if cp > 0xFFFF or pair > 0xFFFF:
            sys.exit("a paired bracket outside the BMP: the table keeps u16")
    for cp, m in mirrors:
        if cp > 0xFFFF or m > 0xFFFF:
            sys.exit("a mirrored character outside the BMP: the table keeps u16")
    out = [HEADER.format(ver=ver),
           array("BMP_STARTS", "u16", [s for s, _ in bmp], 10, 6),
           array("BMP_CLASSES", "u8", [CLASSES.index(c) for _, c in bmp], 24),
           array("REST_STARTS", "u32", [s for s, _ in rest], 8, 7),
           array("REST_CLASSES", "u8", [CLASSES.index(c) for _, c in rest], 24),
           "/// Paired brackets: code point, its pair, 1 if it opens.\n" + array("BRACKETS", "(u16, u16, u8)",
                 [f"({cp:#06x}, {pair:#06x}, {int(kind == 'o')})" for cp, pair, kind in brackets], 4),
           "/// Bidi_Mirroring_Glyph: code point and its mirror.\n" + array("MIRRORS", "(u16, u16)", [f"({cp:#06x}, {m:#06x})" for cp, m in mirrors], 6)]
    return "\n".join(out), len(bmp), len(rest)


def arabic_tables(ucd, ver):
    joining = {}
    for code, value in rows(ucd / "DerivedJoiningType.txt"):
        first, last = span(code)
        for cp in range(first, last + 1):
            joining[cp] = value
    jt = runs([joining.get(cp, "U") for cp in range(0x110000)], *ARABIC)
    forms, lam_alef = {}, {}
    ccc = {}
    for fields in rows(ucd / "UnicodeData.txt"):
        cp = int(fields[0], 16)
        if ARABIC[0] <= cp <= ARABIC[1] and fields[3] != "0":
            ccc[cp] = int(fields[3])
        tag, _, rest = fields[5].partition(" ")
        if tag not in FORMS or not (0xFB50 <= cp <= 0xFDFF or 0xFE70 <= cp <= 0xFEFF):
            continue
        parts = [int(p, 16) for p in rest.split()]
        if len(parts) == 1 and ARABIC[0] <= parts[0] <= ARABIC[1]:
            forms.setdefault(parts[0], [0, 0, 0, 0])[FORMS[tag]] = cp
        elif len(parts) == 2 and parts[0] == LAM and parts[1] in ALEFS and tag in ("<isolated>", "<final>"):
            lam_alef.setdefault(parts[1], [0, 0])[0 if tag == "<isolated>" else 1] = cp
    below = sorted(cp for cp, c in ccc.items() if c in (29, 32, 202, 218, 220, 222))
    out = [HEADER.format(ver=ver),
           array("JOINING_STARTS", "u16", [s for s, _ in jt], 10, 6),
           array("JOINING_TYPES", "u8", [JOININGS.index(t) for _, t in jt], 24),
           "/// Presentation forms of a letter: isolated, final, initial, medial (0: none).\n" + array("FORMS", "(u16, [u16; 4])",
                 [f"({b:#06x}, [{', '.join(f'{x:#06x}' for x in f)}])" for b, f in sorted(forms.items())], 2),
           "/// Lam with an alef: the alef, the isolated and the final ligature.\n" + array("LAM_ALEF", "(u16, [u16; 2])",
                 [f"({a:#06x}, [{f[0]:#06x}, {f[1]:#06x}])" for a, f in sorted(lam_alef.items())], 2),
           "/// Marks of the Arabic blocks drawn below their base (by combining class).\n" + array("BELOW", "u16", below, 10, 6)]
    return "\n".join(out), len(jt), len(forms), len(lam_alef)


def mark_table(ucd, ver):
    marks, first = set(), None
    for fields in rows(ucd / "UnicodeData.txt"):
        cp, name, category = int(fields[0], 16), fields[1], fields[2]
        if name.endswith(", First>"):
            first = cp
            continue
        start = first if name.endswith(", Last>") else cp
        first = None
        if category.startswith("M"):
            marks.update(range(start, cp + 1))
    found = []
    for cp in sorted(marks):
        if found and found[-1][1] == cp - 1:
            found[-1][1] = cp
        else:
            found.append([cp, cp])
    for start, last in found:
        if last - start >= 1 << MARK_SHIFT:
            sys.exit(f"a run of marks from {start:04X} longer than {1 << MARK_SHIFT}")
    out = [HEADER.format(ver=ver),
           "/// The runs of characters of general category M (Mn, Mc, Me), sorted:\n"
           f"/// the first code point shifted left by {MARK_SHIFT}, the run's length less one\n"
           "/// below it.\n"
           + array("MARKS", "u32", [(s << MARK_SHIFT) | (l - s) for s, l in found], 8, 10),
           f"/// How far a run's first code point is shifted in [`MARKS`].\npub(super) const SHIFT: u32 = {MARK_SHIFT};\n"]
    return "\n".join(out), len(marks), len(found)


HEADER = """// Generated by tools/unicode-tables.py from the Unicode Character
// Database {ver} (Unicode License v3, see NOTICE). Do not edit.
"""


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    ucd = Path(sys.argv[1])
    ver = version(ucd / "DerivedBidiClass.txt")
    text, bmp, rest = bidi_tables(ucd, ver)
    BIDI_OUT.parent.mkdir(parents=True, exist_ok=True)
    BIDI_OUT.write_text(text, encoding="utf-8")
    text, jt, forms, lam_alef = arabic_tables(ucd, ver)
    ARABIC_OUT.parent.mkdir(parents=True, exist_ok=True)
    ARABIC_OUT.write_text(text, encoding="utf-8")
    text, marks, mark_runs = mark_table(ucd, ver)
    MARKS_OUT.write_text(text, encoding="utf-8")
    print(f"UCD {ver}: {bmp} + {rest} bidi class runs, {jt} joining runs, "
          f"{forms} letters with forms, {lam_alef} lam-alef ligatures, "
          f"{marks} marks in {mark_runs} runs")


if __name__ == "__main__":
    main()
