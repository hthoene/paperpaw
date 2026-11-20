#!/usr/bin/env python3
"""Checks that the interface's font can draw every UI language pack.

Every character of every message of tools/plugin-files/languages/LANG.ini
must be in the interface's font (Source Sans 3, `fonts-default.source-sans`)
as the device has it: its built-in letters (the `latin` and `ligatures`
sets of paperpaw_text::charset), the icons, and the letters of the script
packs the language's data.toml block requires (`script-<set>`), as
tools/script-pack.py makes them for the interface's font (the set's
characters the font has). Characters that draw nothing (joiners, zero
width space, soft hyphen) and those paperpaw_text stands in for a covered
one pass.

Usage:
    python3 tools/glyph-coverage.py [LANG ...]
        Each character a pack lacks, a line; exit status 1 if any.
"""

import importlib.util
import re
import sys
import tomllib
from pathlib import Path

from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parents[1]
LANGUAGES = ROOT / "tools/plugin-files/languages"
CHARSET = ROOT / "crates/paperpaw-text/src/charset.rs"
SANS = "fonts-default.source-sans"
# Draw nothing: soft hyphen, ZWSP, ZWNJ, ZWJ, word joiner, LRM/RLM, BOM.
INVISIBLE = set("­​‌‍⁠‎‏﻿")
ICONS = range(0xE000, 0xF900)


def rust_char(text):
    text = text.strip("'")
    if text.startswith("\\u{"):
        return chr(int(text[3:-1], 16))
    return {"\\\\": "\\", "\\'": "'"}.get(text, text)


def rust_sets():
    """paperpaw_text::charset's sets by name, and its stand-ins."""
    src = CHARSET.read_text(encoding="utf-8")
    char = r"'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^'])'"
    consts = {}
    for name, body in re.findall(r"const (\w+): &\[RangeInclusive<char>\] = &\[(.*?)\];", src, re.S):
        chars = set()
        for a, b in re.findall(rf"({char})\.\.=({char})", body):
            chars.update(map(chr, range(ord(rust_char(a)), ord(rust_char(b)) + 1)))
        consts[name] = chars
    table = re.search(r"const SETS: .*?= \[(.*?)\];", src, re.S).group(1)
    sets = {n: consts[c] for n, c in re.findall(r'\("([\w-]+)", (\w+)\)', table)}
    stand = {}
    for left, right in re.findall(rf"((?:{char}\s*\|?\s*)+)=> ({char})", src):
        for ch in re.findall(char, left):
            stand[rust_char(ch)] = rust_char(right)
    return sets, stand


def chars_of(spec, sets):
    out = set()
    for part in spec.split(","):
        if part.startswith("@"):
            lines = Path(part[1:]).read_text(encoding="utf-8").splitlines()
            words = " ".join(line.partition("#")[0] for line in lines).replace(",", " ").split()
            out |= chars_of(",".join(words), sets) if words else set()
        elif part.upper().startswith("U+"):
            a, _, b = part[2:].partition("-")
            out.update(map(chr, range(int(a, 16), int(b or a, 16) + 1)))
        else:
            out.update(sets.get(part, ()))
    return out - set("\n\r")


CMAPS = {}


def cmap(path):
    if path not in CMAPS:
        CMAPS[path] = {chr(c) for c in TTFont(path, lazy=True).getBestCmap()}
    return CMAPS[path]


def script_pack():
    spec = importlib.util.spec_from_file_location("script_pack", ROOT / "tools/script-pack.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def pack_letters(name, sp, sets):
    """What `script-<name>` brings the interface's font."""
    entry = sp.SETS[name]
    default = entry[4] or [name]
    out = set()
    for face in entry[1]:
        targets = face[3] if isinstance(face[3], list) else [face[3]]
        if SANS not in targets:
            continue
        own = face[4] if len(face) > 4 else None
        spec = own if own else ",".join(default)
        out |= chars_of(spec, sets) & cmap(face[1])
    return out


def messages(path):
    for n, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if line.startswith(("#", ";", "[")) or " = " not in line:
            continue
        key, _, value = line.partition(" = ")
        if key.strip() in ("name", "native", "code", "direction", "plural"):
            continue
        yield n, key.strip(), value.replace("\\n", "\n").replace("\\t", " ")


def main():
    sets, stand = rust_sets()
    sp = script_pack()
    sans = sp.FONTS / "SourceSans3.ttf"
    builtin = (sets["latin"] | sets["ligatures"]) & cmap(sans) | {" ", "\n"}
    data = tomllib.loads((ROOT / "tools/plugin-files/data.toml").read_text(encoding="utf-8"))
    langs = sys.argv[1:] or sorted(p.stem for p in LANGUAGES.glob("*.ini"))
    bad = 0
    for lang in langs:
        have = set(builtin)
        for req in data.get(f"language-{lang}", {}).get("requires", []):
            if req.startswith("script-"):
                for name in ([req[7:]] if req[7:] in sp.SETS else req[7:].split("-")):
                    have |= pack_letters(name, sp, sets)
        path = LANGUAGES / f"{lang}.ini"
        for n, key, text in messages(path):
            for ch in dict.fromkeys(text):
                if ch in have or ch in INVISIBLE or ord(ch) in ICONS:
                    continue
                if stand.get(ch) in have or stand.get(stand.get(ch)) in have:
                    continue
                print(f"{path.relative_to(ROOT)}:{n}: {key}: U+{ord(ch):04X} {ch!r} not in the interface's font")
                bad += 1
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
