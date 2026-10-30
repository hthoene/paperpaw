#!/usr/bin/env python3
"""Writes, from crates/paperpaw-table/shared.list, the index of the
firmware's shared function tables (crates/paperpaw-table/src/index.rs)
the signature of every entry (crates/paperpaw-table/table.sig) and the
firmware's addresses of them (crates/paperpaw-sdk/src/shared_table.rs), as
the `shared!` declarations in the crates' sources give them.

    python3 tools/shared-table.py           # writes both files
    python3 tools/shared-table.py --check   # fails where one is not current

It also checks the list's rules: entries are only appended, each at a
revision of its family not below the lines before it (a batch of entries
shares one revision), and every entry is declared by exactly one `shared!` (and every
`shared!` is listed). `cargo test -p paperpaw-sdk --test interface`
checks that the blessed list and signatures (interface/) are a prefix
of these.
"""

import hashlib
import pathlib
import re
import sys

REPO = pathlib.Path(__file__).resolve().parent.parent
CRATE = REPO / "crates" / "paperpaw-table"
LIST = CRATE / "shared.list"
INDEX = CRATE / "src" / "index.rs"
SIG = CRATE / "table.sig"
ADDRESSES = REPO / "crates" / "paperpaw-sdk" / "src" / "shared_table.rs"
FAMILIES = ["text", "fonts", "ui", "graphics", "contribution"]
COUNT = 8


def parse():
    """The entries `(family, revision, name, path)` and the bumps."""
    entries, bumps, last = [], [], {}
    for number, raw in enumerate(LIST.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.split("#", 1)[0].split()
        if not line:
            continue
        if line[0] == "bump" and len(line) == 3:
            family, revision = line[1], int(line[2])
            bumps.append((family, revision))
        elif len(line) == 4:
            family, revision, name, path = line[0], int(line[1]), line[2], line[3]
            entries.append((family, revision, name, path))
        else:
            sys.exit(f"shared.list:{number}: not an entry or a bump: {raw}")
        if family not in FAMILIES:
            sys.exit(f"shared.list:{number}: no family {family}")
        if revision < last.get(family, 0):
            sys.exit(f"shared.list:{number}: revision {revision} of {family} "
                     f"is below {last[family]}")
        last[family] = revision
    names = [(f, n) for f, _, n, _ in entries]
    if len(set(names)) != len(names):
        sys.exit("shared.list: an index name is there twice in one family")
    return entries, bumps


def declarations():
    """`(family, name) -> signature` of every `shared!` declaration."""
    found = {}
    marker = re.compile(r"@\s*(\w+)\s*::\s*(\w+)")
    for path in sorted(REPO.glob("crates/*/src/**/*.rs")):
        if CRATE in path.parents:
            continue
        # Comments (documentation's examples) declare nothing.
        text = re.sub(r"(?m)^\s*//.*$", "", path.read_text(encoding="utf-8"))
        if "shared!" not in text:
            continue
        for m in marker.finditer(text):
            head = text.rfind("fn ", 0, m.start())
            body = text.find("{", head)
            if head < 0 or body < 0 or body > m.start():
                continue
            signature = " ".join(text[head:body].split())
            key = (m.group(1), m.group(2))
            if key in found:
                sys.exit(f"{path}: {key[0]}::{key[1]} declared twice")
            found[key] = signature
    return found


def table_hash(entries, bumps, found):
    """Eight hex digits naming the tables of this build, for logs and
    crash reports (`Api::build`): the digest of every entry's place and
    signature, and of the bumps."""
    text = "".join(f"{f} {r} {n} {found[(f, n)]}\n" for f, r, n, _ in entries)
    text += "".join(f"bump {f} {r}\n" for f, r in bumps)
    return hashlib.sha256(text.encode("utf-8")).hexdigest()[:8]


def index_rs(entries, bumps, hash_):
    revisions = [0] * COUNT
    oldest = [0] * COUNT
    for family, revision, _, _ in entries:
        i = FAMILIES.index(family)
        revisions[i] = max(revisions[i], revision)
        oldest[i] = oldest[i] or 1
    for family, revision in bumps:
        i = FAMILIES.index(family)
        revisions[i] = max(revisions[i], revision)
        oldest[i] = revision
    out = ["// Made by tools/shared-table.py from shared.list: do not edit.", "",
           "/// The revision of each family (`shared.list`).",
           f"pub const REVISIONS: crate::Revisions = {revisions};",
           "/// The oldest revision of each family whose plugins still run.",
           f"pub const OLDEST: crate::Revisions = {oldest};",
           "/// Eight hex digits naming the tables of this build, for logs.",
           f'pub const HASH: &str = "{hash_}";']
    for family in FAMILIES:
        out += ["", f"/// The {family} family's table.", f"pub mod {family} {{",
                f"    /// The family.",
                f"    pub const FAMILY: crate::Family = crate::Family::{family.upper()};"]
        mine = [e for e in entries if e[0] == family]
        for i, (_, revision, name, path) in enumerate(mine):
            out.append(f"    /// `{path}` (revision {revision}).")
            out.append(f"    pub const {name}: usize = {i};")
        out.append(f"    /// How many entries the table has.")
        out.append(f"    pub const COUNT: usize = {len(mine)};")
        out.append("}")
    return "\n".join(out) + "\n"


def table_sig(entries, found):
    out = ["# The signature of every shared function, in the order of",
           "# shared.list (tools/shared-table.py). Part of the plugin interface."]
    for family, revision, name, _ in entries:
        out.append(f"{family} {revision} {name}: {found[(family, name)]}")
    return "\n".join(out) + "\n"


def addresses_rs(entries):
    """The firmware's side: the address of every shared function, per
    family, in the order of the list (`paperpaw_sdk::shared`)."""
    out = ["// Made by tools/shared-table.py from shared.list: do not edit.",
           "#![cfg_attr(rustfmt, rustfmt::skip)]", ""]
    for family in FAMILIES:
        mine = [e for e in entries if e[0] == family]
        if not mine:
            continue
        out.append(f"pub(super) static {family.upper()}: super::Addresses<{len(mine)}> "
                   "= super::Addresses([")
        for _, _, name, path in mine:
            out.append(f"    {path} as *const (), // {name}")
        out += ["]);", ""]
    out += ["/// The table of family number `n`, if the firmware shares it.",
            "pub(super) const fn of(n: usize) -> paperpaw_table::Table {",
            "    match n {"]
    for i, family in enumerate(FAMILIES):
        if any(e[0] == family for e in entries):
            out.append(f"        {i} => paperpaw_table::Table({family.upper()}.0.as_ptr()),")
    out += ["        _ => paperpaw_table::Table::NONE,", "    }", "}"]
    return "\n".join(out) + "\n"


def main():
    entries, bumps = parse()
    found = declarations()
    listed = {(f, n) for f, _, n, _ in entries}
    missing = sorted(listed - set(found))
    extra = sorted(set(found) - listed)
    if missing or extra:
        sys.exit(f"shared.list and the sources differ: listed, not declared: "
                 f"{missing}; declared, not listed: {extra}")
    files = {INDEX: index_rs(entries, bumps, table_hash(entries, bumps, found)), SIG: table_sig(entries, found),
             ADDRESSES: addresses_rs(entries)}
    if "--check" in sys.argv:
        stale = [p for p, text in files.items()
                 if not p.exists() or p.read_text(encoding="utf-8") != text]
        if stale:
            sys.exit("not current, write them with python3 tools/shared-table.py: "
                     + ", ".join(str(p.relative_to(REPO)) for p in stale))
        return
    for path, text in files.items():
        path.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
