#!/usr/bin/env python3
"""Writes, from crates/paperpaw-table/shared.list, the index of the
firmware's shared function tables (crates/paperpaw-table/src/index.rs)
the signature of every entry (crates/paperpaw-table/table.sig), the
firmware's addresses of them (crates/paperpaw-sdk/src/shared_table.rs), as
the `shared!` declarations in the crates' sources give them, and a digest
of each entry's body and of the functions of its crate it calls
(crates/paperpaw-table/table.body): `--check` fails where a body changed
and the digests were not written anew in the same commit, so that every
change plugins built before would run shows (ADR 0026).

    python3 tools/shared-table.py           # writes both files
    python3 tools/shared-table.py --check   # fails where one is not current
    python3 tools/shared-table.py --appended-to OLD.list
        # fails unless this list is OLD.list (the last released one) with
        # entries appended at a higher revision of their family

It also checks the list's rules: entries are only appended, each at a
revision of its family not below the lines before it (a batch of entries
shares one revision, until the list is released: then every entry appended
needs a revision above the released list's, see `--appended-to`: a plugin
built with the new entry says that revision, and firmware that has it
only at the old one would call past the end of its table), and every entry is declared by exactly one `shared!` (and every
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
BODY = CRATE / "table.body"
ADDRESSES = REPO / "crates" / "paperpaw-sdk" / "src" / "shared_table.rs"
# Families 0 to 6 by number (6, `widgets`, was kept for the firmware's
# own `core` and `alloc` code: that goes to a chained family when there
# is one); 7 is kept for the chain to 8 and up
# (paperpaw_table::FAMILIES).
FAMILIES = ["text", "fonts", "ui", "graphics", "contribution", "data", "widgets"]
assert len(FAMILIES) <= 7, "family 7 is kept for the chain"
COUNT = 8


def parse():
    """The entries `(family, revision, name, path)` and the bumps."""
    return parse_text(LIST.read_text(encoding="utf-8"), "shared.list")


def parse_text(text, label):
    entries, bumps, last = [], [], {}
    for number, raw in enumerate(text.splitlines(), 1):
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
            sys.exit(f"{label}:{number}: not an entry or a bump: {raw}")
        if family not in FAMILIES:
            sys.exit(f"{label}:{number}: no family {family}")
        if revision < last.get(family, 0):
            sys.exit(f"{label}:{number}: revision {revision} of {family} "
                     f"is below {last[family]}")
        last[family] = revision
    names = [(f, n) for f, _, n, _ in entries]
    if len(set(names)) != len(names):
        sys.exit(f"{label}: an index name is there twice in one family")
    return entries, bumps


def late_appends(released, now):
    """What is wrong with `now` as the continuation of the `released` list
    (both `parse_text` results): the problems, as text. The released
    entries must be the first ones, unchanged, and every entry after them
    needs a revision above anything its family had in the released list."""
    (old, old_bumps), (new, _) = released, now
    problems = []
    if new[:len(old)] != old:
        problems.append("the released entries are not the first ones, unchanged")
    top = {}
    for family, revision, _, _ in old:
        top[family] = max(top.get(family, 0), revision)
    for family, revision in old_bumps:
        top[family] = max(top.get(family, 0), revision)
    for family, revision, name, _ in new[len(old):]:
        if revision <= top.get(family, 0):
            problems.append(
                f"{family}::{name} is appended at revision {revision}, not above "
                f"the released {top[family]}: firmware at that revision would "
                "have no such entry (give it the next revision)")
    return problems


def declarations():
    """`(family, name) -> signature` of every `shared!` declaration."""
    found = {}
    # The end of a `shared!` body: `} @ family::INDEX` (not a binding
    # such as `n @ Value::Int(_)`).
    marker = re.compile(r"\}\s*@\s*(\w+)\s*::\s*(\w+)")
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


# A name defined more often than this in a crate (`new`, `get`, `len`) is
# not followed into its functions: which one a call means is not known
# from the text, and following all of them would make every hash change
# with any change in the crate.
MAX_SAME_NAME = 2
# The names a shared body calls that its crate defines more than
# `MAX_SAME_NAME` times, by crate: their functions are not followed (see
# above). Any other name that crosses the limit fails: a new function named
# like one a shared body calls would otherwise drop that body from the digest
# without a trace (#732). Rename the new function, or add the name here.
# A name listed here that no shared body skips any more fails too.
SAME_NAME_SKIPPED = {
    "paperpaw-graphics": frozenset({
        "format",
    }),
    "paperpaw-sdk": frozenset({
        "bits", "button", "cells", "choice", "color", "command", "default", "device",
        "draw", "draw_lines", "each", "env", "fit", "fits", "focused", "font", "forget",
        "frame", "height", "ink", "into_iter", "keep", "key", "line", "list", "make",
        "name", "new", "on", "read", "read_at", "rows", "separator", "set", "settings",
        "shows", "size", "state", "switch", "targets", "text", "title", "widgets",
        "with", "wrap",
    }),
    "paperpaw-text": frozenset({
        "advance", "anchors", "class", "default", "finish", "glyph", "has_char", "kern",
        "level", "new", "read_at",
    }),
}
# Copies of the plugin-boundary codecs that tools/abi-gen.py writes: the ABI
# lock pins every byte of them. Index their names without parsing their bodies:
# a reachable call defined only there is refused rather than silently unhashed.
GENERATED = "// Generated by tools/abi-gen.py; do not edit."
# The interface-7 plugin facade (`src/native/`) runs only in plugins, never in
# a shared body: its many small accessors (`groups`, `menus`, `is_last`) would
# otherwise make the names shared bodies call ambiguous, moving digests with no
# body changed. Indexed by name only, as generated sources: a shared body that
# reaches a function defined only there is refused, never silently unhashed.
NATIVE = "native"
# A path into that facade (`native::x`, `crate::native::x`; not
# `paperpaw_native::`), and a `use` that imports from it.
NATIVE_PATH = re.compile(r"(?<![\w])native\s*::")
NATIVE_USE = re.compile(r"\buse\b[^;]*(?<![\w])native\b[^;]*;")
# A public re-export of a facade item (`pub use crate::native::x`,
# `pub(crate) use native::{x}`): outside `src/native.rs` it would let other
# files call facade code by a name that is not the facade's path.
NATIVE_REEXPORT = re.compile(r"\bpub\b(?:\s*\([^)]*\))?\s*use\b[^;]*"
                             r"(?:(?<![\w:])native\s*::|\b(?:crate|super|self)\s*::\s*native\b)")


def is_native(path, crate=None):
    """Whether `path` lies in a crate's `src/native/`."""
    parts = path.relative_to(crate).parts if crate else path.parts
    return any(a == "src" and b == NATIVE for a, b in zip(parts, parts[1:]))


CALL = re.compile(r"(?<![\w.:])(?:\.\s*)?([a-z_][a-z0-9_]*)\s*(?:::<[^()]*?>)?\s*\(")
METHOD = re.compile(r"\.\s*([a-z_][a-z0-9_]*)\s*(?:::<[^()]*?>)?\s*\(")
PATHCALL = re.compile(r"::\s*([a-z_][a-z0-9_]*)\s*(?:::<[^()]*?>)?\s*\(")
KEYWORDS = {"if", "while", "match", "for", "return", "loop", "in", "as", "fn", "let", "move", "unsafe"}
# Methods of `core` and `alloc` (iterators, options, slices, strings,
# numbers) that a crate may also name a function of its own: a call of
# them is mostly the standard one.
STANDARD = set("""
all any by_ref chain chars char_indices bytes clamp cloned collect copied count cycle enumerate
filter filter_map find find_map first flat_map flatten fold for_each fuse inspect last map
map_while max max_by max_by_key min min_by min_by_key next nth partition peekable position
product rev rposition scan skip skip_while step_by sum take take_while unzip zip
and_then as_deref as_mut as_ref expect filter is_none is_some is_some_and is_none_or map_or
map_or_else ok ok_or ok_or_else or or_else unwrap unwrap_or unwrap_or_default unwrap_or_else
xor zip get get_mut iter iter_mut len is_empty push pop insert remove retain truncate extend
contains starts_with ends_with split split_once splitn rsplit trim trim_start trim_end lines
strip_prefix strip_suffix to_string to_owned into from clone eq ne cmp partial_cmp
abs pow sqrt floor ceil round signum saturating_sub saturating_add checked_sub checked_add
wrapping_add wrapping_sub min_value max_value as_str as_bytes with_capacity reserve clear
binary_search sort sort_by sort_by_key windows chunks join concat repeat replace parse
to_lowercase to_uppercase is_char_boundary rfind find_char hash write write_str fmt
then then_some
""".split())

# Methods of workspace crates that are not hashed, which shared bodies call
# and the native facade also names: `format` is `paperpaw_i18n::I18n::format`
# (and the facade's `Declarations::format`), `tone` is
# `paperpaw_text::Ink::tone` (and the facade's `fonts::Ink::tone`, slot 41),
# `luma` is `paperpaw_platform::Color::luma` (and the facade's
# `image::ImageReady::luma`, the picture recipe's grid).
# As for a standard name, the call is that method only where src/native/
# alone defines the name; unlike a standard name it is still followed where
# a hashed function has it.
EXTERNAL = {"format", "luma", "tone"}


def block_end(text, i):
    """The index after the block whose `{` is at `text[i]`: strings,
    characters and comments skipped."""
    depth, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            i = text.find("\n", i)
            if i < 0:
                return n
            continue
        if text.startswith("/*", i):
            i = text.find("*/", i) + 2
            continue
        raw = re.match(r'r(#*)"', text[i:i + 8]) if c == "r" and not (text[i - 1].isalnum() or text[i - 1] == "_") else None
        if raw:
            end = '"' + raw.group(1)
            i = text.index(end, i + len(raw.group(0))) + len(end)
            continue
        if c == '"':
            j = i + 1
            while text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "'":
            m = re.match(r"'(\\.[^']*|[^\\'])'", text[i:])
            if m:
                i += len(m.group(0))
                continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return n


def normal(code):
    """`code` without comments and with its spaces made one: a change of
    formatting or of a comment is no change of the body."""
    code = re.sub(r"(?m)//.*$", "", code)
    code = re.sub(r"/\*.*?\*/", "", code, flags=re.S)
    return " ".join(code.split())


def crate_functions(crate, generated=None, native_users=None):
    """`name -> [(file, body), ...]` of handwritten functions in `crate`.
    If supplied, `generated` receives names and files, without parsing bodies,
    of generated and native sources, and `native_users` the other files that
    import from the native facade.
    """
    found = {}
    head = re.compile(r"\bfn\s+([a-z_][a-z0-9_]*)\b")
    for path in sorted(crate.glob("src/**/*.rs")):
        text = path.read_text(encoding="utf-8")
        parts = path.relative_to(crate).parts
        native = parts[:2] == ("src", NATIVE)
        # The facade's root module calls its submodules by bare paths.
        root = parts == ("src", NATIVE + ".rs")
        if not native and not root and NATIVE_REEXPORT.search(normal(text)):
            sys.exit(f"{path}: re-exports native facade items outside src/native.rs; "
                     "shared bodies could reach them unhashed")
        if native_users is not None and not native and (root or NATIVE_USE.search(normal(text))):
            native_users.add(path)
        if text.startswith(GENERATED) or native:
            if generated is not None:
                for m in head.finditer(text):
                    generated.setdefault(m.group(1), []).append(path)
            continue
        for m in head.finditer(text):
            body = text.find("{", m.end())
            semi = text.find(";", m.end())
            if body < 0 or (0 <= semi < body):
                continue
            found.setdefault(m.group(1), []).append(
                (path, normal(text[body:block_end(text, body)])))
    return found


def called(body, include_standard=False):
    """The names `body` calls: functions, methods and paths' last parts."""
    names = set()
    for pattern in (CALL, METHOD, PATHCALL):
        names.update(m.group(1) for m in pattern.finditer(body))
    names -= KEYWORDS
    return names if include_standard else names - STANDARD


def bodies():
    """`(family, name) -> (crate, file, body)` of every `shared!` declaration:
    the body as written, from its `{` to the `}` before `@`."""
    found = {}
    marker = re.compile(r"\}\s*@\s*(\w+)\s*::\s*(\w+)")
    for path in sorted(REPO.glob("crates/*/src/**/*.rs")):
        if CRATE in path.parents:
            continue
        text = re.sub(r"(?m)^\s*//.*$", "", path.read_text(encoding="utf-8"))
        if "shared!" not in text:
            continue
        crate = path.relative_to(REPO).parents[-3]
        for m in marker.finditer(text):
            head = text.rfind("fn ", 0, m.start())
            body = text.find("{", head)
            if head < 0 or body < 0 or body > m.start():
                continue
            found[(m.group(1), m.group(2))] = (REPO / crate, path, text[body:m.start() + 1])
    return found


def reach(functions, file, body, generated=None, native_users=frozenset(),
          skipped=None):
    """The bodies of the functions `body` (in `file`) reaches by calls,
    from `functions` (`crate_functions`): a name defined in the file of
    the code that calls it means that file's functions, another one those
    of the crate where it has at most `MAX_SAME_NAME`. A name with more is
    refused unless it is in `skipped` (the crate's `SAME_NAME_SKIPPED`, a
    set the caller passes and sees filled with the names met: with None,
    every such name is refused).

    Native facade code is never hashed, so reaching it is refused: a
    `native::` path in a reached body, or a call of a name the facade
    defines from a file that imports from it (the name could mean the
    facade's function while another one is hashed)."""
    reached, todo, seen = set(), [(file, normal(body))], set()
    while todo:
        at, code = todo.pop()
        if NATIVE_PATH.search(code):
            sys.exit(f"{at}: shared body reaches the native facade by a native:: path; "
                     "src/native/ is never hashed")
        for callee in sorted(called(code, include_standard=True)):
            defs = functions.get(callee, [])
            # Checked before standard names are skipped: a generated function
            # named `len` is still refused. Only a standard name the native
            # facade alone defines (`retain`) is the standard method.
            if not defs and generated and callee in generated and not (
                    (callee in STANDARD or callee in EXTERNAL)
                    and all(is_native(path) for path in generated[callee])):
                files = ", ".join(str(path) for path in generated[callee])
                sys.exit(f"{at}: shared body calls {callee}, defined only in "
                         f"generated or native source: {files}; its body cannot be skipped")
            # Also before standard names: in a file that imports from the
            # facade, a standard name (`retain`) may mean the facade's method.
            if at in native_users and generated and any(
                    is_native(path) for path in generated.get(callee, [])):
                sys.exit(f"{at}: shared body calls {callee}, which src/native/ also "
                         "defines, from a file that imports from native; rename it")
            if callee in STANDARD:
                continue
            local = [d for d in defs if d[0] == at]
            if local:
                defs = local
            elif len(defs) > MAX_SAME_NAME:
                if skipped is None or callee not in skipped[0]:
                    sys.exit(f"{at}: shared body calls {callee}, which its crate defines "
                             f"{len(defs)} times (more than {MAX_SAME_NAME}): its bodies "
                             "would drop out of the digest. Rename the function that "
                             "made it cross, or add the name to SAME_NAME_SKIPPED")
                skipped[1].add(callee)
                continue
            for d in defs:
                if d not in seen:
                    seen.add(d)
                    reached.add(d[1])
                    todo.append(d)
    return reached


def body_hashes(entries):
    """Twelve hex digits per entry: a digest of its body and of the
    functions of its crate it reaches (`reach`)."""
    found = bodies()
    crates = {}
    out = {}
    met = {}
    for family, _, name, _ in entries:
        crate, file, body = found[(family, name)]
        if crate not in crates:
            generated, users = {}, set()
            crates[crate] = (crate_functions(crate, generated, users), generated, users)
        functions, generated, users = crates[crate]
        listed = SAME_NAME_SKIPPED.get(crate.name, frozenset())
        skipped = (listed, met.setdefault(crate.name, set()))
        reached = reach(functions, file, body, generated, users, skipped)
        text = normal(body) + "\n" + "\n".join(sorted(reached))
        out[(family, name)] = hashlib.sha256(text.encode("utf-8")).hexdigest()[:12]
    stale = sorted(f"{crate} {name}" for crate, names in SAME_NAME_SKIPPED.items()
                   for name in names - met.get(crate, set()))
    if stale:
        sys.exit("SAME_NAME_SKIPPED lists names no shared body skips any more "
                 f"(remove them): {', '.join(stale)}")
    return out


def table_body(entries):
    hashes = body_hashes(entries)
    out = ["# A digest of each shared function's body and of the functions of",
           "# its crate that it calls, in the order of shared.list",
           "# (tools/shared-table.py). Plugins built before run these bodies: a",
           "# line that changes is a change they see, so its commit says which",
           "# case of ADR 0026 (\"Shared functions run the firmware's body\") it is",
           "# and appends a `bump` line to shared.list where that says so. Not",
           "# part of the plugin interface (never blessed)."]
    for family, revision, name, _ in entries:
        out.append(f"{family} {revision} {name}: {hashes[(family, name)]}")
    return "\n".join(out) + "\n"


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
    if "--appended-to" in sys.argv:
        old = pathlib.Path(sys.argv[sys.argv.index("--appended-to") + 1])
        problems = late_appends(parse_text(old.read_text(encoding="utf-8"), str(old)), parse())
        if problems:
            sys.exit("shared.list against the released list:\n  " + "\n  ".join(problems))
        return
    entries, bumps = parse()
    found = declarations()
    listed = {(f, n) for f, _, n, _ in entries}
    missing = sorted(listed - set(found))
    extra = sorted(set(found) - listed)
    if missing or extra:
        sys.exit(f"shared.list and the sources differ: listed, not declared: "
                 f"{missing}; declared, not listed: {extra}")
    files = {INDEX: index_rs(entries, bumps, table_hash(entries, bumps, found)), SIG: table_sig(entries, found),
             ADDRESSES: addresses_rs(entries), BODY: table_body(entries)}
    if "--check" in sys.argv:
        stale = [p for p, text in files.items()
                 if not p.exists() or p.read_text(encoding="utf-8") != text]
        if stale:
            hint = ""
            if BODY in stale:
                before = BODY.read_text(encoding="utf-8").splitlines() if BODY.exists() else []
                changed = sorted(set(files[BODY].splitlines()) - set(before))
                hint = "\nChanged: " + ", ".join(line.split(":")[0] for line in changed if not line.startswith("#"))
                hint += ("\nA shared function's body changed (table.body): plugins built "
                        "before run it. Say in the commit which case of ADR 0026 "
                        "(\"Shared functions run the firmware's body\") it is, append a "
                        "`bump` line to shared.list where that says so, and write the "
                        "files anew.")
            sys.exit("not current, write them with python3 tools/shared-table.py: "
                     + ", ".join(str(p.relative_to(REPO)) for p in stale) + hint)
        return
    for path, text in files.items():
        path.write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
