#!/usr/bin/env python3
"""Writes crates/paperpaw-lang/src/tables.rs: the scripts (ISO 15924) and
regions (ISO 3166-1 alpha-2 and the UN M49 areas BCP 47 allows) whose
index a `Lang` keeps, and the language codes that have a shorter or a
newer form.

The script and region tables are part of the plugin interface (a `Lang`
holds an index into them): entries are only ever appended. The first
run sorted them; later runs keep the order of the file there is and
append what is new, sorted. `cargo test -p paperpaw-sdk --test
interface` checks that the blessed list is a prefix of the current one.

    python3 tools/lang-tables.py      # needs pycountry (pip install pycountry)

The codes are those of the ISO standards, as the iso-codes data that
pycountry carries lists them.
"""

import pathlib
import re

import pycountry

REPO = pathlib.Path(__file__).resolve().parent.parent
OUT = REPO / "crates" / "paperpaw-lang" / "src" / "tables.rs"

# UN M49 areas that BCP 47 allows as regions (RFC 5646 2.2.4), as used.
M49 = ["001", "002", "003", "005", "009", "011", "013", "014", "015",
       "017", "018", "019", "021", "029", "030", "034", "035", "039",
       "053", "054", "057", "061", "142", "143", "145", "150", "151",
       "154", "155", "202", "419"]

# Deprecated language codes and what replaced them (IANA registry).
DEPRECATED = {"iw": "he", "in": "id", "ji": "yi", "jw": "jv", "mo": "ro"}


def existing(name):
    """The entries of table `name` in the file there is, in its order."""
    if not OUT.exists():
        return []
    text = OUT.read_text(encoding="utf-8")
    m = re.search(rf"pub\(crate\) const {name}: &\[\[u8; \d\]\] = &\[(.*?)\];", text, re.S)
    return re.findall(r'\*b"([^"]+)"', m.group(1)) if m else []


def appended(name, codes):
    old = existing(name)
    return old + sorted(set(codes) - set(old))


def main():
    scripts = appended("SCRIPTS", [s.alpha_4 for s in pycountry.scripts])
    regions = appended("REGIONS", [c.alpha_2 + " " for c in pycountry.countries]
                       + M49)
    assert len(scripts) < 256 and len(regions) < 512
    shorter = {}
    for lang in pycountry.languages:
        two = getattr(lang, "alpha_2", None)
        if two:
            shorter[lang.alpha_3] = two
            bib = getattr(lang, "bibliographic", None)
            if bib:
                shorter[bib] = two
    shorter.update(DEPRECATED)
    canon = sorted(shorter.items())
    out = ["// Made by tools/lang-tables.py from the ISO code lists: do not edit.",
           "// SCRIPTS and REGIONS only grow at their end (plugin interface).",
           "",
           "/// ISO 15924 scripts; a `Lang` keeps the index + 1 (0: none).",
           f"pub(crate) const SCRIPTS: &[[u8; 4]] = &["]
    out += [f'    *b"{s}",' for s in scripts]
    out += ["];", "",
            "/// Regions: ISO 3166-1 alpha-2 (padded with a space) and UN M49",
            "/// areas; a `Lang` keeps the index + 1 (0: none).",
            "pub(crate) const REGIONS: &[[u8; 3]] = &["]
    out += [f'    *b"{r}",' for r in regions]
    out += ["];", "",
            "/// Language codes with another canonical form (ISO 639-2 codes of",
            "/// languages with a two-letter code, deprecated codes), sorted.",
            "pub(crate) const CANONICAL: &[(&str, &str)] = &["]
    out += [f'    ("{a}", "{b}"),' for a, b in canon]
    out += ["];", ""]
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text("\n".join(out), encoding="utf-8")
    print(f"{len(scripts)} scripts, {len(regions)} regions, {len(canon)} canonical forms")


if __name__ == "__main__":
    main()
