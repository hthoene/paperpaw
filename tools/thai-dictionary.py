"""The Thai words the line breaker finds in books (#313).

Thai is written without spaces between words. The packs of the plugin
`script-thai` carry a table of words (slot 6 of a pack, see
`crates/paperpaw-text/src/bitmap.rs`), which paperpaw-text reads into
memory the first time it breaks Thai text into lines and holds while the
pack is open: a line then breaks between two words it finds there
(longest match, see `crates/paperpaw-text/src/linebreak/thai.rs`).

The words are `tools/thai-dictionary.txt` (one a line): the most frequent
words of the Thai National Corpus (PyThaiNLP's `tnc_freq.txt`, CC0 1.0)
and those of the interface (`tools/thai-words.txt`), of 2 to 15 letters.

    python3 tools/thai-dictionary.py make TNC_FREQ
        Writes tools/thai-dictionary.txt from PyThaiNLP's frequency list
        (pythainlp/corpus/tnc_freq.txt).
    python3 tools/thai-dictionary.py add PACK ...
        Puts the table into each pack (`.ppfp` of version 2), as
        tools/script-pack.py does for the Thai packs.
    python3 tools/thai-dictionary.py size
        Prints the words and the table's bytes.

The table (version 1): its head (version, 3 reserved, length u32), the
word count u16, the block count u16, the offset u16 of each block from
the first block's start, then the words sorted by their letters, 16 a
block. A letter is one byte, its code point less U+0E00; a word is one
byte, the letters it shares with the word before in the high four bits
(0 for a block's first) and the count of those that follow in the low
four, then those.
"""

import re
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORDS = ROOT / "tools/thai-dictionary.txt"
UI_WORDS = ROOT / "tools/thai-words.txt"
# How many of the corpus' most frequent words: 92.7 % of its words in
# running text, a table of some 19 KB.
FREQUENT = 4000
LONGEST = 15
BLOCK = 16
SLOT = 6
VERSION = 1
LETTERS = re.compile(r"^[ก-ฺเ-๎]+$")
# A word never starts with a sign that stays on the letter before it, nor
# ends with a vowel written before its letter.
FOLLOWS = set("ะัาำิีึืฺุู็่้๊๋์ํ๎ๅๆฯ")
LEADS = set("เแโใไ")
HEADER = """\
# Thai words for the line breaker (#313, tools/thai-dictionary.py): the
# {n} most frequent words of the Thai National Corpus (PyThaiNLP's
# tnc_freq.txt, CC0 1.0) and the words of tools/thai-words.txt, of 2 to
# 15 letters. Made with `python3 tools/thai-dictionary.py make TNC_FREQ`.
"""


def fits(word):
    return (2 <= len(word) <= LONGEST and LETTERS.match(word)
            and word[0] not in FOLLOWS and word[-1] not in LEADS)


def lines(path):
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            yield line


def make(tnc):
    ranked = []
    for line in Path(tnc).read_text(encoding="utf-8").splitlines():
        word, _, count = line.partition("\t")
        if fits(word):
            ranked.append((-int(count), word))
    ranked.sort()
    words = {w for _, w in ranked[:FREQUENT]} | {w for w in lines(UI_WORDS) if fits(w)}
    text = HEADER.format(n=FREQUENT) + "".join(f"{w}\n" for w in sorted(words, key=key))
    WORDS.write_text(text, encoding="utf-8", newline="\n")


def key(word):
    return bytes(ord(c) - 0xE00 for c in word)


def table(words=None):
    """The table's bytes, its head included."""
    keys = sorted({key(w) for w in (words or lines(WORDS)) if fits(w)})
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
    return struct.pack("<B3xI", VERSION, 8 + len(data)) + data


def add(path, made=None):
    """Appends the table to the pack at `path` and points slot 6 at it."""
    pack = bytearray(Path(path).read_bytes())
    if pack[:4] != b"PPFP" or pack[4] != 2:
        sys.exit(f"{path}: not a pack with tables (version 2)")
    strikes = struct.unpack_from("<H", pack, 8)[0]
    directory = 12 + 32 * strikes
    at = len(pack) + (-len(pack)) % 4
    pack += bytes(at - len(pack)) + (made or table())
    struct.pack_into("<I", pack, directory + 4 * SLOT, at)
    Path(path).write_bytes(bytes(pack))


def main(argv):
    if argv[:1] == ["make"] and len(argv) == 2:
        make(argv[1])
    elif argv[:1] == ["add"] and len(argv) > 1:
        made = table()
        for path in argv[1:]:
            add(path, made)
    elif argv == ["size"]:
        print(f"{sum(1 for w in lines(WORDS) if fits(w))} words, {len(table())} bytes")
    else:
        sys.exit(__doc__)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
