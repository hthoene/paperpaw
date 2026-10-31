#!/usr/bin/env python3
"""Writes what code plugins see of each crate whose types cross between
a plugin and PaperPaw (ADR 0026), as the plugin interface's test
(`cargo test -p paperpaw-sdk --test interface`) compares it:

- `api.txt`, the crate's public API, as cargo-public-api lists it;
- `shape.txt`, the shape of its types, from rustdoc's JSON with every
  feature on: the fields, variants and trait items (with their
  signatures) of its public types and of every private type their fields
  and trait items reach, in the order the source
  has them, private fields too, each with its type (functions' pointers
  with their ABI); and the values of its public constants. Order,
  private fields and values change how a type is laid out, how a trait's
  table of functions is made and what both sides take a value for,
  which `api.txt` does not show: it is sorted, public only and without
  values.

    python3 tools/interface.py           # writes both files of each crate
    python3 tools/interface.py --check   # fails where one is not current

The crates are those `crates/paperpaw-sdk/interface/crates.txt` names.
Needs the nightly Rust that `NIGHTLY` names, with cargo-public-api of the
version `PUBLIC_API` names (as CI installs it).
"""

import json
import os
import pathlib
import subprocess
import sys
import tempfile

REPO = pathlib.Path(__file__).resolve().parent.parent
NIGHTLY = "nightly-2026-10-17"
PUBLIC_API = "0.52.0"
CRATES = REPO / "crates" / "paperpaw-sdk" / "interface" / "crates.txt"
HEADER = (
    "# The shape of this crate's types and the values of its constants, as\n"
    "# tools/interface.py writes them: fields, variants and trait items in\n"
    "# the source's order, private fields and the private types they reach\n"
    "# too, every feature on (ADR 0026).\n"
)


def crates():
    lines = CRATES.read_text(encoding="utf-8").splitlines()
    return [l.strip() for l in lines if l.strip() and not l.startswith("#")]


def check_public_api():
    """cargo-public-api is the version CI has: another may list otherwise."""
    run = subprocess.run(
        ["cargo", f"+{NIGHTLY}", "public-api", "--version"],
        cwd=REPO, capture_output=True, text=True, encoding="utf-8", check=True,
    )
    if PUBLIC_API not in run.stdout.split():
        sys.exit(f"cargo-public-api is {run.stdout.strip()}, not {PUBLIC_API}: "
                 f"cargo install cargo-public-api@{PUBLIC_API} --locked")


def api(crate):
    """The crate's public API, as `api.txt` keeps it."""
    run = subprocess.run(
        ["cargo", f"+{NIGHTLY}", "public-api", "-p", crate, "-sss"],
        cwd=REPO, capture_output=True, text=True, encoding="utf-8", check=True,
    )
    return run.stdout.replace("\r\n", "\n")


def rustdoc(crate, target):
    """The crate's rustdoc JSON, private items included, every feature on."""
    subprocess.run(
        ["cargo", f"+{NIGHTLY}", "rustdoc", "-q", "-p", crate, "--lib",
         "--all-features", "--",
         "-Z", "unstable-options", "--output-format", "json",
         "--document-private-items"],
        cwd=REPO, check=True, env={**os.environ, "CARGO_TARGET_DIR": target},
    )
    path = pathlib.Path(target) / "doc" / (crate.replace("-", "_") + ".json")
    return json.loads(path.read_text(encoding="utf-8"))


def strip_ids(value):
    """`value` without rustdoc's ids, which change from build to build."""
    if isinstance(value, dict):
        return {k: strip_ids(v) for k, v in value.items() if k != "id"}
    if isinstance(value, list):
        return [strip_ids(v) for v in value]
    return value


def raw(value):
    return json.dumps(strip_ids(value), sort_keys=True)


def is_repr(attr):
    """Whether an attribute changes the layout: `repr` in any form."""
    return "repr" in (attr if isinstance(attr, str) else json.dumps(attr))


def ids_in(value):
    """The ids of the paths a type names, at any depth."""
    if isinstance(value, dict):
        for k, v in value.items():
            if k in ("resolved_path", "trait") and isinstance(v, dict) and "id" in v:
                yield v["id"]
            yield from ids_in(v)
    elif isinstance(value, list):
        for v in value:
            yield from ids_in(v)


def abi(header):
    """A function's pointer's ABI and kind: `unsafe extern "C" `."""
    text = "unsafe " if header.get("is_unsafe") else ""
    which = header.get("abi", "Rust")
    if which != "Rust":
        (name, _), = which.items() if isinstance(which, dict) else ((which, None),)
        text += f'extern "{name}" '
        if isinstance(which, dict) and (which[name] or {}).get("unwind"):
            text += "unwind "
    return text


class Shapes:
    """The types and constants of one crate's rustdoc JSON, as lines."""

    def __init__(self, doc):
        self.index = doc["index"]
        self.paths = doc["paths"]

    def path(self, id_, fallback):
        entry = self.paths.get(str(id_))
        return "::".join(entry["path"]) if entry else fallback

    def name(self, path):
        return self.path(path.get("id"), path["path"]) + self.args(path.get("args"))

    def ty(self, t):
        (kind, v), = t.items()
        if kind in ("primitive", "generic"):
            return v
        if kind == "resolved_path":
            return self.name(v)
        if kind == "borrowed_ref":
            life = v["lifetime"] + " " if v.get("lifetime") else ""
            return "&" + life + ("mut " if v["is_mutable"] else "") + self.ty(v["type"])
        if kind == "raw_pointer":
            return ("*mut " if v["is_mutable"] else "*const ") + self.ty(v["type"])
        if kind == "slice":
            return "[" + self.ty(v) + "]"
        if kind == "array":
            return "[" + self.ty(v["type"]) + "; " + str(v["len"]) + "]"
        if kind == "tuple":
            return "(" + ", ".join(self.ty(x) for x in v) + ")"
        if kind == "dyn_trait":
            bounds = [self.name(b["trait"]) for b in v["traits"]]
            bounds += [v["lifetime"]] if v.get("lifetime") else []
            return "dyn " + " + ".join(bounds)
        if kind == "function_pointer":
            sig = v["sig"]
            out = " -> " + self.ty(sig["output"]) if sig.get("output") else ""
            params = [p["name"] for p in v.get("generic_params", [])]
            hrtb = "for<" + ", ".join(params) + "> " if params else ""
            variadic = ", ..." if sig.get("is_c_variadic") else ""
            inputs = ", ".join(self.ty(x[1]) for x in sig["inputs"])
            return hrtb + abi(v.get("header", {})) + "fn(" + inputs + variadic + ")" + out
        if kind == "qualified_path":
            return "<" + self.ty(v["self_type"]) + ">::" + v["name"]
        return raw(t)

    def args(self, args):
        if not args:
            return ""
        if "angle_bracketed" in args:
            parts = []
            for arg in args["angle_bracketed"]["args"]:
                (kind, v), = arg.items()
                parts.append(self.ty(v) if kind == "type" else str(v))
            parts += [raw(c) for c in args["angle_bracketed"].get("constraints", [])]
            return "<" + ", ".join(parts) + ">" if parts else ""
        if "parenthesized" in args:
            p = args["parenthesized"]
            out = " -> " + self.ty(p["output"]) if p.get("output") else ""
            return "(" + ", ".join(self.ty(x) for x in p["inputs"]) + ")" + out
        return raw(args)

    def field(self, id_):
        if id_ is None:
            return "_"
        item = self.index[str(id_)]
        return item["name"] + ": " + self.ty(item["inner"]["struct_field"])

    def fields(self, kind):
        """A struct's or a variant's fields: `{ a: T }`, `(0: T)` or none."""
        if not isinstance(kind, dict):
            return ""
        (form, v), = kind.items()
        if form == "tuple":
            return "(" + ", ".join(self.field(f) for f in v) + ")"
        return " { " + ", ".join(self.field(f) for f in v["fields"]) + " }"

    def variant(self, id_):
        item = self.index[str(id_)]
        v = item["inner"]["variant"]
        text = item["name"] + self.fields(v["kind"])
        if v.get("discriminant"):
            text += " = " + v["discriminant"]["expr"]
        return text

    def member(self, id_):
        """A trait's item: its kind and name, and a function's signature
        (a private trait's is in no `api.txt`, and its table holds it)."""
        item = self.index[str(id_)]
        (kind, v), = item["inner"].items()
        text = kind + " " + item["name"]
        if kind == "function":
            params = [p["name"] for p in v.get("generics", {}).get("params", [])]
            if params:
                text += "<" + ", ".join(params) + ">"
            sig = v["sig"]
            text += "(" + ", ".join(self.ty(x[1]) for x in sig["inputs"]) + ")"
            if sig.get("output"):
                text += " -> " + self.ty(sig["output"])
        return text

    def is_type(self, item):
        return item["crate_id"] == 0 and next(iter(item["inner"])) in (
            "struct", "enum", "union", "trait")

    def is_alias(self, item):
        return item["crate_id"] == 0 and next(iter(item["inner"])) == "type_alias"

    def reached(self, item):
        """The ids of the types an item's fields and variants name, and
        those its trait's items take and give: a value a trait object's
        function returns crosses as a field's does. A type alias is
        followed to the types it names (`type Hook = fn(&mut Band)`)."""
        (kind, v), = item["inner"].items()
        if kind == "type_alias":
            yield from ids_in(v["type"])
            return
        if kind == "trait":
            for member in v["items"]:
                inner = self.index.get(str(member), {}).get("inner", {})
                if "function" in inner:
                    sig = inner["function"]["sig"]
                    yield from ids_in([x[1] for x in sig["inputs"]])
                    yield from ids_in(sig.get("output"))
            return
        members = []
        if kind == "struct" and isinstance(v["kind"], dict):
            (form, f), = v["kind"].items()
            members = f if form == "tuple" else f["fields"]
        elif kind == "union":
            members = v["fields"]
        elif kind == "enum":
            for variant in v["variants"]:
                vk = self.index[str(variant)]["inner"]["variant"]["kind"]
                if isinstance(vk, dict):
                    (form, f), = vk.items()
                    members += f if form == "tuple" else f["fields"]
        for member in members:
            if member is not None:
                yield from ids_in(self.index[str(member)]["inner"])

    def wanted(self):
        """The public types, and every private one their fields reach."""
        todo = [i for i in self.index.values()
                if self.is_type(i) and i["visibility"] == "public"]
        seen = {i["id"] for i in todo}
        while todo:
            item = todo.pop()
            for id_ in self.reached(item):
                other = self.index.get(str(id_))
                if (other and (self.is_type(other) or self.is_alias(other))
                        and other["id"] not in seen):
                    seen.add(other["id"])
                    todo.append(other)
        return [self.index[str(i)] for i in seen if self.is_type(self.index[str(i)])]

    def line(self, item):
        (kind, v), = item["inner"].items()
        head = kind + " " + self.path(item["id"], item["name"])
        params = [p["name"] for p in v.get("generics", {}).get("params", [])]
        if params:
            head += "<" + ", ".join(params) + ">"
        head += "".join(" #" + raw(a) for a in item["attrs"] if is_repr(a))
        if kind == "struct":
            body = self.fields(v["kind"])
        elif kind == "union":
            body = " { " + ", ".join(self.field(f) for f in v["fields"]) + " }"
        elif kind == "enum":
            body = " { " + ", ".join(self.variant(x) for x in v["variants"]) + " }"
        else:
            body = " { " + ", ".join(self.member(x) for x in v["items"]) + " }"
        return head + body

    def constants(self):
        """`const <path>: <type> = <value>` of the public constants, and of
        the associated ones of every `impl`."""
        lines = []
        for item in self.index.values():
            if item["crate_id"] != 0:
                continue
            (kind, v), = item["inner"].items()
            if kind == "constant" and item["visibility"] == "public":
                path = self.path(item["id"], item["name"])
                lines.append(f"const {path}: {self.ty(v['type'])} = {value(v['const'], item)}")
            elif kind == "impl":
                owner = self.ty(v["for"])
                trait = f" ({self.name(v['trait'])})" if v.get("trait") else ""
                for member in v["items"]:
                    m = self.index.get(str(member))
                    if not m or "assoc_const" not in m["inner"]:
                        continue
                    if not trait and m["visibility"] != "public":
                        continue
                    c = m["inner"]["assoc_const"]
                    said = c.get("value")
                    if not said or said == "_":
                        said = source_value(m)
                    lines.append(f"const {owner}::{m['name']}{trait}: {self.ty(c['type'])} = "
                                 f"{said}")
        return lines


def value(const, item=None):
    """A constant's value as rustdoc says it: evaluated where it can, else
    the expression it is written as."""
    if const.get("value"):
        return const["value"]
    if const.get("expr") and const["expr"] != "_":
        return const["expr"]
    return source_value(item) if item else "?"


def source_value(item):
    """What a constant is set to in its source, where rustdoc does not say
    (`Uses(1 << 0)`): the text after its `=`, its spaces made one."""
    span = item.get("span")
    if not span:
        return "?"
    path = REPO / span["filename"].replace("\\", "/")
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError:
        return "?"
    text = " ".join(lines[span["begin"][0] - 1:span["end"][0]])
    at = text.find(f"const {item['name']}")
    if at < 0 or "=" not in text[at:]:
        return "?"
    said = text[at:].split("=", 1)[1].split(";", 1)[0]
    return " ".join(said.split()) or "?"


def shape(crate, target):
    doc = rustdoc(crate, target)
    shapes = Shapes(doc)
    lines = {shapes.line(i) for i in shapes.wanted()} | set(shapes.constants())
    return HEADER + "".join(line + "\n" for line in sorted(lines))


def main():
    check = "--check" in sys.argv[1:]
    check_public_api()
    stale = []
    with tempfile.TemporaryDirectory(prefix="interface-") as target:
        for crate in crates():
            for name, text in (("api.txt", api(crate)), ("shape.txt", shape(crate, target))):
                path = REPO / "crates" / crate / name
                old = path.read_text(encoding="utf-8").replace("\r\n", "\n") if path.exists() else None
                if old == text:
                    continue
                if check:
                    stale.append(str(path.relative_to(REPO)))
                else:
                    path.write_text(text, encoding="utf-8", newline="\n")
                    print("wrote", path.relative_to(REPO))
    if stale:
        sys.exit("not current; write them with python3 tools/interface.py:\n  " + "\n  ".join(stale))


if __name__ == "__main__":
    main()
