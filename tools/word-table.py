"""The Khmer, Lao and Burmese words the line breaker finds in books (#314,
#397).

Khmer, Lao and Burmese are written without spaces between words, as Thai
is (`tools/thai-dictionary.py`, #313). The packs of the plugins
`script-khmer`, `script-lao` and `script-myanmar` carry a table of words (slot 6 of a pack,
see `crates/paperpaw-text/src/bitmap.rs`) in the Thai table's format, the
script in its head: a line then breaks between two words found there
(longest match, `crates/paperpaw-text/src/linebreak/thai.rs`).

The words are `tools/khmer-words.txt`, `tools/lao-words.txt` and
`tools/myanmar-words.txt` (one a line): the words of ICU's word lists
(`khmerdict.txt`, `laodict.txt`, `burmesedict.txt` of
icu4c/source/data/brkitr/dictionaries; Unicode License V3, the Lao list
also under its authors' BSD-style licence, NOTICE) of 2 to 15 letters
found most often in the articles of the Khmer, Lao and Burmese Wikipedias (a dump
`pages-articles.xml.bz2`, its runs of letters cut by longest match).

    python3 tools/word-table.py make SCRIPT ICU_LIST WIKI_DUMP
        Writes the script's word list.
    python3 tools/word-table.py add SCRIPT PACK ...
        Puts its table into each pack (`.ppfp` of version 2), as
        tools/script-pack.py does.
    python3 tools/word-table.py size SCRIPT
        Prints the words and the table's bytes.

The table (version 1): its head (version, the script: 0 Thai, 1 Lao,
2 Khmer, 3 Myanmar, 2 reserved, length u32), the word count u16, the block count
u16, the offset u16 of each block from the first block's start, then the
words sorted by their letters, 16 a block. A letter is one byte, its code
point less the block's first (U+0E80 for Lao, U+1780 for Khmer, U+1000
for Burmese); a word
is one byte, the letters it shares with the word before in the high four
bits (0 for a block's first) and the count of those that follow in the
low four, then those.
"""

import bz2
import collections
import re
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LONGEST = 15
BLOCK = 16
SLOT = 6
VERSION = 1

# Per script: its number in the head, the first code point of its block,
# the letters words are made of, the signs a word never starts with (they
# stay on the letter before) and those it never ends with, the most words.
SCRIPTS = {
    "lao": (1, 0x0E80, "ກ-ຽເ-໎ໜ-ໟ",
            "ຯະ-ຽໆ່-໎", "ເ-ໄ", 3000),
    "khmer": (2, 0x1780, "ក-៓ៗ៝",
              "឴-៓ៗ៝", "្", 3000),
    "myanmar": (3, 0x1000, "က-ဿ၌-ၙ",
                "ါ-ှၖ-ၙ", "္", 2000),
}
HEADER = """\
# {Name} words for the line breaker (#314, tools/word-table.py): the
# {n} words of ICU's {list} (Unicode License V3) of 2 to 15 letters found
# most often in the articles of the {Name} Wikipedia. Made with
# `python3 tools/word-table.py make {script} {list} WIKI_DUMP`.
"""


def words_file(script):
    return ROOT / f"tools/{script}-words.txt"


def fits(script, word):
    _, _, letters, follows, leads, _ = SCRIPTS[script]
    return (2 <= len(word) <= LONGEST and re.fullmatch(f"[{letters}]+", word)
            and not re.match(f"[{follows}]", word[0]) and not re.match(f"[{leads}]", word[-1]))


def lines(path):
    for line in Path(path).read_text(encoding="utf-8-sig").splitlines():
        line = line.split("\t")[0].strip()
        if line and not line.startswith("#"):
            yield line


def make(script, icu, dump):
    known = {w for w in lines(icu) if fits(script, w)}
    run = re.compile(f"[{SCRIPTS[script][2]}]+")
    counts = collections.Counter()
    with bz2.open(dump, "rt", encoding="utf-8") as f:
        for line in f:
            for letters in run.findall(line):
                at = 0
                while at < len(letters):
                    for n in range(min(LONGEST, len(letters) - at), 1, -1):
                        if letters[at:at + n] in known:
                            counts[letters[at:at + n]] += 1
                            at += n
                            break
                    else:
                        at += 1
    most = SCRIPTS[script][5]
    words = sorted((w for w, _ in counts.most_common(most)), key=lambda w: key(script, w))
    text = HEADER.format(Name=script.capitalize(), n=len(words), list=Path(icu).name,
                         script=script)
    words_file(script).write_text(text + "".join(f"{w}\n" for w in words), encoding="utf-8",
                                  newline="\n")


def key(script, word):
    base = SCRIPTS[script][1]
    return bytes(ord(c) - base for c in word)


def table(script):
    """The table's bytes, its head included."""
    keys = sorted({key(script, w) for w in lines(words_file(script)) if fits(script, w)})
    blocks, body = [], bytearray()
    for n, word in enumerate(keys):
        shared = 0
        if n % BLOCK == 0:
            blocks.append(len(body))
        else:
            before = keys[n - 1]
            while shared < len(before) and shared < len(word) - 1 and word[shared] == before[shared]:
                shared += 1
        body.append(shared << 4 | (len(word) - shared))
        body += word[shared:]
    if len(body) > 0xFFFF:
        sys.exit("the words take more than 64 KiB")
    head = struct.pack("<HH", len(keys), len(blocks)) + b"".join(
        struct.pack("<H", b) for b in blocks)
    data = head + body
    return struct.pack("<BB2xI", VERSION, SCRIPTS[script][0], 8 + len(data)) + data


def add(path, made):
    """Appends the table to the pack at `path` and points slot 6 at it."""
    pack = bytearray(Path(path).read_bytes())
    if pack[:4] != b"PPFP" or pack[4] != 2:
        sys.exit(f"{path}: not a pack with tables (version 2)")
    strikes = struct.unpack_from("<H", pack, 8)[0]
    directory = 12 + 32 * strikes
    at = len(pack) + (-len(pack)) % 4
    pack += bytes(at - len(pack)) + made
    struct.pack_into("<I", pack, directory + 4 * SLOT, at)
    Path(path).write_bytes(bytes(pack))


def main(argv):
    if len(argv) < 2 or argv[1] not in SCRIPTS:
        sys.exit(__doc__)
    script = argv[1]
    if argv[0] == "make" and len(argv) == 4:
        make(script, argv[2], argv[3])
    elif argv[0] == "add" and len(argv) > 2:
        made = table(script)
        for path in argv[2:]:
            add(path, made)
    elif argv[0] == "size" and len(argv) == 2:
        n = sum(1 for w in lines(words_file(script)) if fits(script, w))
        print(f"{n} words, {len(table(script))} bytes")
    else:
        sys.exit(__doc__)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
