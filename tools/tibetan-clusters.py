"""The Tibetan clusters the packs of `script-tibetan` keep shaped (#314).

Noto Serif Tibetan draws its stacks (a consonant with the letters
subjoined under it, and its vowel signs) with some 1,100 substitutions
chosen by context: some 230 KB as a pack keeps them, more than a pack read
from a card may hold in memory. A pack keeps instead what those make of
the clusters text uses most, shaped when the pack is made
(`crates/paperpaw-text/src/indic/clusters.rs`): these, the most frequent
in the articles of the Tibetan Wikipedia (99.8 % of its clusters).

    python3 tools/tibetan-clusters.py WIKI_DUMP
        Writes crates/paperpaw-text/src/indic/tibetan_clusters.txt from a
        dump `bowiki-...-pages-articles.xml.bz2`.
"""

import bz2
import collections
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "crates/paperpaw-text/src/indic/tibetan_clusters.txt"
MOST = 600
# A consonant, then subjoined letters, vowel signs and the marks on them.
CLUSTER = re.compile("[ཀ-ཬྈ-ྌ][ཱ༹༵༷-྄྆྇"
                     "ྍ-ྼ࿆]*")
HEADER = """\
# Tibetan clusters the packs keep shaped (#314, tools/tibetan-clusters.py):
# the {n} most frequent in the articles of the Tibetan Wikipedia.
"""


def main(argv):
    if len(argv) != 1:
        sys.exit(__doc__)
    counts = collections.Counter()
    with bz2.open(argv[0], "rt", encoding="utf-8") as f:
        for line in f:
            counts.update(CLUSTER.findall(line))
    top = [c for c, _ in counts.most_common(MOST)]
    OUT.write_text(HEADER.format(n=len(top)) + "".join(f"{c}\n" for c in top), encoding="utf-8",
                   newline="\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
