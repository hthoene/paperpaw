#!/usr/bin/env python3
"""Makes the shared function table's two generated files from crates/paperpaw-native/shared.list
(prototype of option B2, #44):

- crates/paperpaw-native/src/shared_index.rs: the index of each entry and their number, which a
  plugin's stubs and `Entry::needs` use;
- crates/paperpaw-sdk/src/shared_table.rs: the firmware's table, the address of each function in the
  same order.

    python3 tools/shared-table.py           # writes both
    python3 tools/shared-table.py --check   # fails where one is not current

A test (`cargo test -p paperpaw-sdk shared`) checks that every `@ NAME` of a `shared!` in the sources
is on the list, and the other way round.
"""
import pathlib, sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
LIST = ROOT / "crates/paperpaw-native/shared.list"
INDEX = ROOT / "crates/paperpaw-native/src/shared_index.rs"
TABLE = ROOT / "crates/paperpaw-sdk/src/shared_table.rs"

def entries():
    out = []
    for line in LIST.read_text().splitlines():
        line = line.split("#", 1)[0].strip()
        if line:
            name, path = line.split()
            out.append((name, path))
    names = [n for n, _ in out]
    assert len(set(names)) == len(names), "a name twice"
    return out

def index_rs(es):
    s = "// Made by tools/shared-table.py from shared.list: do not edit.\n"
    for i, (n, _) in enumerate(es):
        s += f"pub const {n}: usize = {i};\n"
    s += f"/// How many entries the table has.\npub const COUNT: usize = {len(es)};\n"
    return s

def table_rs(es):
    s = "// Made by tools/shared-table.py from shared.list: do not edit.\n"
    s += f"pub(super) const COUNT: usize = {len(es)};\n\n"
    s += "pub(super) static ADDRESSES: super::Addresses<COUNT> = super::Addresses([\n"
    for n, p in es:
        s += f"    {p} as *const (), // {n}\n"
    s += "]);\n"
    return s

def sites():
    """Every `shared! { ... } @ NAME` in the sources: (name, function, enclosing impl type or None, file)."""
    import re
    found = []
    for krate in ("paperpaw-text", "paperpaw-sdk"):
        for f in sorted((ROOT / "crates" / krate / "src").rglob("*.rs")):
            lines = f.read_text().split("\n")
            for n, line in enumerate(lines):
                m = re.search(r"\} @ (\w+)\s*$", line)
                if not m:
                    continue
                # back to the macro's opening line, then the function and the impl above it
                k = n
                while k >= 0 and "paperpaw_native::shared! {" not in lines[k]:
                    k -= 1
                name = fn = None
                for j in range(k, n + 1):
                    mm = re.search(r"\bfn (\w+)", lines[j])
                    if mm:
                        fn = mm.group(1)
                        break
                indent = len(lines[k]) - len(lines[k].lstrip())
                ty = None
                for j in range(k, -1, -1):
                    mm = re.match(r"^(\s*)impl(?:<[^>]*>)? (?:[\w:]+ for )?(\w+)", lines[j])
                    if mm and len(mm.group(1)) < indent:
                        ty = mm.group(2)
                        break
                found.append((m.group(1), fn, ty, f.relative_to(ROOT)))
    return found

def check_sites(es):
    """The name at each site must be the function the list says is at that index."""
    listed = dict(es)
    problems = []
    for name, fn, ty, f in sites():
        path = listed.get(name)
        if path is None:
            problems.append(f"{f}: {name} is not on the list")
            continue
        tail = path.split("::")
        want = tail[-2:] if ty else tail[-1:]
        have = [ty, fn] if ty else [fn]
        if want != have:
            problems.append(f"{f}: {name} is listed as {path}, but the site is {'::'.join(have)}")
    seen = {n for n, *_ in sites()}
    problems += [f"{n} on the list has no site" for n in listed if n not in seen]
    return problems

es = entries()
files = {INDEX: index_rs(es), TABLE: table_rs(es)}
if "--check" in sys.argv:
    problems = check_sites(es)
    if problems:
        sys.exit("\n".join(problems))
    bad = [str(p) for p, t in files.items() if not p.exists() or p.read_text() != t]
    if bad:
        sys.exit("not current: " + ", ".join(bad) + " (python3 tools/shared-table.py)")
else:
    for p, t in files.items():
        p.write_text(t)
    print(len(es), "entries")
