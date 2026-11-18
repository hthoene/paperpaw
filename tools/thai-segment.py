"""Marks the word boundaries of Thai UI texts for the line breaker (#190).

Thai is written without spaces between words, so a line may be broken
only where a word ends. The firmware breaks lines at U+200B (zero width
space: a place to break that draws nothing and takes no room), so the
catalog of the Thai language pack holds one after each word: PaperPaw
needs no word list or breaker of its own for the interface (0 bytes of
firmware, nothing loaded unless Thai is the language), and the pack
grows by 3 bytes a break (49 KB of 537 KB, about 10 %; it is read from the
card as it is needed).

The words are those of `tools/thai-words.txt` (one a line): the common
words of the interface found by longest matching (fewest letters in no
word, then fewest words), so a new text needs its new words in the list.
A run of letters that no word explains stays whole (never broken
inside), and `check` names it.

    python3 tools/thai-segment.py apply [FILE ...]
        Puts the zero width spaces into the messages of FILE (default:
        tools/plugin-files/languages/th.ini), taking out those there, so
        the source keeps one form. Idempotent.
    python3 tools/thai-segment.py check [FILE ...]
        Exit status 1 and the lines that differ from what `apply` makes
        (a text without breaks, or a break in a word), and the letter
        runs of 8 or more no word explains.
    python3 tools/thai-segment.py split TEXT
        Prints TEXT with its breaks shown as `|`.

Words are never broken inside placeholders (`{count}`), between a letter
and the mark that stays on it (vowel signs, tone marks, ๆ, ฯ), or after a
leading vowel (เ แ โ ใ ไ).
"""

import re
import sys
from functools import lru_cache
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WORDS = ROOT / "tools/thai-words.txt"
SOURCE = ROOT / "tools/plugin-files/languages/th.ini"
ZWSP = "​"

THAI = re.compile(r"[ก-๛]+")
PLACEHOLDER = re.compile(r"\{[^{}]*\}")
# Characters that stay with the letter before them: the vowels above and
# below, sara a, sara aa and am, the tone marks, maiyamok, paiyannoi.
FOLLOWS = set("ะัาำิีึืฺุู็่้๊๋์ํ๎ๅๆฯ")
# The leading vowels stay with the letter after them.
LEADS = set("เแโใไ")
LONGEST = 24


def words(path=WORDS):
    out = set()
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line and not line.startswith("#"):
            out.add(line)
    return out


def may_break(run, i):
    """Whether the run may be broken before `run[i]`."""
    return run[i] not in FOLLOWS and run[i - 1] not in LEADS


def tokens(run, known):
    """The words of one run of Thai letters: by fewest letters in no word,
    then fewest words. Letters no word explains join into one token."""

    @lru_cache(maxsize=None)
    def best(i):
        # (unknown letters, tokens, the tokens) of run[i:]
        if i == len(run):
            return (0, 0, ())
        options = []
        # Longest first: of equal splits, the one with the longer first word.
        for j in range(min(len(run), i + LONGEST), i, -1):
            if run[i:j] in known and (j == len(run) or may_break(run, j)):
                u, n, rest = best(j)
                options.append((u, n + 1, (run[i:j],) + rest))
        # No word here: the letters up to the next place a word may start.
        j = i + 1
        while j < len(run) and not (may_break(run, j) and best(j)[0] < len(run) - j):
            j += 1
        u, n, rest = best(j)
        options.append((u + (j - i), n + 1, (run[i:j],) + rest))
        return min(options, key=lambda o: (o[0], o[1]))

    return list(best(0)[2])


def segment(text, known):
    """`text` with a zero width space between the words of its Thai runs."""
    text = text.replace(ZWSP, "")
    out, at = [], 0
    for hole in PLACEHOLDER.finditer(text):
        out.append(mark(text[at:hole.start()], known))
        out.append(hole.group())
        at = hole.end()
    out.append(mark(text[at:], known))
    return "".join(out)


def mark(text, known):
    return THAI.sub(lambda m: ZWSP.join(tokens(m.group(), known)), text)


def unexplained(text, known):
    """The runs of 8 or more letters that stay whole: words to add."""
    found = []
    text = PLACEHOLDER.sub(" ", text.replace(ZWSP, ""))
    for run in THAI.findall(text):
        found += [t for t in tokens(run, known) if len(t) >= 8 and t not in known]
    return found


def messages(path):
    """(line number, key, value) of the catalog's message lines."""
    section = None
    for n, line in enumerate(Path(path).read_text(encoding="utf-8").splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("["):
            section = stripped.strip("[]")
        elif section == "messages" and "=" in stripped and not stripped.startswith("#"):
            key, _, value = line.partition("=")
            yield n, key.strip(), value.strip()


def main(argv):
    if not argv or argv[0] not in ("apply", "check", "split"):
        sys.exit(__doc__)
    known = words()
    if argv[0] == "split":
        print(segment(" ".join(argv[1:]), known).replace(ZWSP, "|"))
        return 0
    files = argv[1:] or [SOURCE]
    bad = 0
    for file in files:
        lines = Path(file).read_text(encoding="utf-8").splitlines()
        for n, key, value in messages(file):
            made = segment(value, known)
            if argv[0] == "apply":
                lines[n - 1] = f"{key} = {made}"
            elif made != value:
                bad += 1
                print(f"{file}:{n}: {key}: breaks differ: {made.replace(ZWSP, '|')}")
            if argv[0] == "check":
                for run in unexplained(value, known):
                    print(f"{file}:{n}: {key}: not in tools/thai-words.txt: {run}")
        if argv[0] == "apply":
            Path(file).write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
