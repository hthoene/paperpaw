"""Writes the notices that every copy of PaperPaw must carry, from the
lockfiles: NOTICE at the top of the repository, and the licences the app
shows (Settings, About PaperPaw), one file for the firmware of the Xteink
readers (plugins/about/data/licences-firmware.md) and one for the apps
(Android, the desktop emulator, the web emulator:
plugins/about/data/licences-app.md).

What ships is read with `cargo metadata`, per shipped target, following
normal dependencies only (no build scripts, proc macros or dev
dependencies): the firmware's workspace (shells/xteink) and the store
plugins with code (tools/plugin-files) for the readers' chips, the root
workspace's shells for Windows, Linux, macOS, Android and the web. Each
third-party crate is listed under the licence it is used under (MIT where
it offers it), with the copyright lines of its licence files (or its
authors, where they hold none); each licence's text follows once.
PaperPaw's own crates are PaperPaw's (LICENSE). What is no crate (fonts,
icons, hyphenation patterns, Mbed TLS, ESP-IDF's libraries, the FreeInk
and OpenX4 code) is in tools/licences/NOTICE.in, by hand.

    python tools/licences/build.py           # writes them
    python tools/licences/build.py --check   # fails where they are not
                                             # what the lockfiles say (CI)

The texts keep their words; in the app's files their lines are joined
into paragraphs, so the reader sets them like a book.
"""
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TEXTS = os.path.join(ROOT, "tools", "licences", "texts")

# What ships, by product: (workspace manifest, root packages or None for
# every member, targets).
CHIPS = ["riscv32imc-unknown-none-elf", "xtensa-esp32s3-none-elf"]
PRODUCTS = {
    "firmware": [
        ("shells/xteink/Cargo.toml", None, CHIPS),
        ("tools/plugin-files7/Cargo.toml", None, CHIPS),
    ],
    "app": [
        ("Cargo.toml", ["paperpaw-emulator", "paperpaw-android", "paperpaw-web"], [
            "x86_64-pc-windows-msvc",
            "x86_64-unknown-linux-gnu",
            "aarch64-apple-darwin",
            "aarch64-linux-android",
            "wasm32-unknown-unknown",
        ]),
    ],
}

# The licence a crate is used under where it offers several: the first of
# these it offers.
PREFER = [
    "MIT", "Apache-2.0", "0BSD", "ISC", "BSD-2-Clause", "BSD-3-Clause", "Zlib",
    "Unicode-3.0", "CDLA-Permissive-2.0",
]
NAMES = {
    "MIT": "MIT License",
    "Apache-2.0": "Apache License 2.0",
    "0BSD": "BSD Zero Clause License",
    "ISC": "ISC License",
    "BSD-2-Clause": "BSD 2-Clause License",
    "BSD-3-Clause": "BSD 3-Clause License",
    "Zlib": "zlib License",
    "Unicode-3.0": "Unicode License v3",
    "CDLA-Permissive-2.0": "Community Data License Agreement Permissive 2.0",
}


def read(path):
    with open(os.path.join(ROOT, path), encoding="utf-8") as f:
        return f.read().strip("\n")


def metadata(manifest, target):
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--all-features",
         "--filter-platform", target, "--manifest-path", os.path.join(ROOT, manifest)],
        capture_output=True, check=True,
    )
    return json.loads(out.stdout.decode("utf-8"))


def shipped(product):
    """The third-party packages `product` ships, by (name, version)."""
    found = {}
    for manifest, roots, targets in PRODUCTS[product]:
        for target in targets:
            meta = metadata(manifest, target)
            packages = {p["id"]: p for p in meta["packages"]}
            nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
            todo = [
                p["id"] for p in meta["packages"]
                if p["id"] in meta["workspace_members"] and (roots is None or p["name"] in roots)
            ]
            seen = set()
            while todo:
                at = todo.pop()
                if at in seen:
                    continue
                seen.add(at)
                for dep in nodes[at]["deps"]:
                    if any(kind["kind"] is None for kind in dep["dep_kinds"]):
                        todo.append(dep["pkg"])
            for at in seen:
                p = packages[at]
                path = p["manifest_path"].replace("\\", "/")
                # PaperPaw's own crates are PaperPaw's; its patched copies
                # of others' crates are theirs.
                if p["source"] is None and "/patches/" not in path:
                    continue
                found[(p["name"], p["version"])] = p
    return found


def choose(expression):
    """The licences a crate is used under: one of each `AND` part, the
    first of PREFER that part offers."""
    expression = (expression or "").replace("/", " OR ")
    chosen = []
    for part in re.split(r"\s+AND\s+", expression):
        offers = [
            re.sub(r"\s+WITH\s+.*", "", o).strip(" ()")
            for o in re.split(r"\s+OR\s+", part.strip(" ()"))
        ]
        pick = next((p for p in PREFER if p in offers), None)
        if pick is None:
            raise SystemExit(f"no licence of {PREFER} in `{expression}`: add its text and name")
        chosen.append(pick)
    return chosen


LICENCE_FILE = re.compile(r"(?i)^(licen[cs]e|copying|copyright|notice|unlicense)")
# A copyright line: "Copyright", perhaps "(c)", then a year or a name;
# not the Apache licence's words about copyright notices.
COPYRIGHT = re.compile(
    r"^(Copyright|COPYRIGHT|copyright)(\s+(\(c\)|\(C\)|©))?:?\s+(\d{4}|[A-Z©]|the\b)"
    r"|^(\(c\)|\(C\)|©)\s*\d{4}"
)
TEMPLATE = re.compile(r"(?i)\[yyyy\]|<year>|\{yyyy\}|\[name of|<copyright|<owner>|\[fullname\]")


def copyrights(p):
    """The copyright lines of `p`'s licence files, or its authors."""
    folder = os.path.dirname(p["manifest_path"])
    lines = []
    for name in sorted(os.listdir(folder)):
        path = os.path.join(folder, name)
        if not LICENCE_FILE.match(name) or not os.path.isfile(path):
            continue
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                line = line.strip().lstrip("/#*!-").strip()
                if COPYRIGHT.match(line) and not TEMPLATE.search(line):
                    line = line[:200]
                    if line not in lines:
                        lines.append(line)
    if not lines:
        authors = [re.sub(r"\s*<[^>]*>", "", a).strip() for a in p.get("authors") or []]
        authors = [a for a in authors if a]
        who = ", ".join(authors) if authors else f"the {p['name']} contributors"
        repository = p.get("repository")
        lines.append(f"Copyright {who}" + (f" ({repository})" if repository else ""))
    return lines


def notices(p):
    """The NOTICE files of a crate used under the Apache licence."""
    folder = os.path.dirname(p["manifest_path"])
    out = []
    for name in sorted(os.listdir(folder)):
        if re.match(r"(?i)^notice", name) and os.path.isfile(os.path.join(folder, name)):
            with open(os.path.join(folder, name), encoding="utf-8", errors="replace") as f:
                out.append(f.read().strip())
    return out


def text_of(licence):
    if licence == "Apache-2.0":
        return read("LICENSE")
    return read(os.path.join("tools", "licences", "texts", f"{licence}.txt"))


def groups(packages):
    """Licence id: the crates used under it, as (name, version, expression,
    copyright lines, notices)."""
    by = {}
    for (name, version), p in sorted(packages.items()):
        chosen = choose(p["license"])
        for licence in chosen:
            by.setdefault(licence, []).append(
                (name, version, p["license"], copyrights(p),
                 notices(p) if licence == "Apache-2.0" else []))
    return {k: by[k] for k in PREFER if k in by}


def paragraphs(text):
    """The text's paragraphs (separated by empty lines) on one line each;
    lines that start a list item stay apart."""
    out = []
    for block in re.split(r"\n\s*\n", text):
        lines = [l.strip() for l in block.splitlines() if l.strip()]
        joined = []
        for line in lines:
            if joined and not re.match(r"^(- |\* |\([a-z0-9]\) |[0-9]+\. |#+ )", line):
                joined[-1] += " " + line
            else:
                joined.append(line)
        # Numbered sections keep their numbers: a no-break space after
        # the number, so Markdown takes them for no list.
        joined = [re.sub(r"^([0-9]+\.) ", "\\1\u00a0", line) for line in joined]
        # Headings inside a text are no chapters of the book.
        joined = [re.sub(r"^#+ ", "", line) for line in joined]
        out.append("\n\n".join(joined))
    return "\n\n".join(out)


def notice_text(packages):
    """NOTICE: the part by hand, then the crates, by licence, each text
    once (the Apache licence's is LICENSE)."""
    out = [read("tools/licences/NOTICE.in"), "",
           "Third-party crates in the firmware and the apps, by the licence each is",
           "used under (generated by tools/licences/build.py from the lockfiles):"]
    for licence, crates in groups(packages).items():
        out += ["", "=" * 72, f"{NAMES[licence]} ({licence})", "=" * 72, ""]
        for name, version, expression, lines, notes in crates:
            used = "" if expression.strip() == licence else f", used under {licence}"
            out.append(f"{name} {version} ({expression}{used})")
            out += [f"    {line}" for line in lines]
            for note in notes:
                out += [f"    {line}" if line else "" for line in note.splitlines()]
        out.append("")
        if licence == "Apache-2.0":
            out.append("The licence's text is in the LICENSE file.")
        else:
            out.append(text_of(licence))
    return "\n".join(out).rstrip() + "\n"


def app_text(product, packages):
    """The licences as the app shows them for `product`, a chapter each."""
    def notices(path):
        return re.findall(r"```text\n(.*?)\n```", read(path), re.S)

    german = notices("plugins/hyphenation-de/patterns/README.md")
    english = notices("plugins/hyphenation/patterns/README.md")
    parts = [
        ("Notice", read("tools/licences/NOTICE.in")),
        ("PaperPaw: Apache License 2.0", read("LICENSE")),
        ("Literata: SIL Open Font License 1.1", read("plugins/fonts-default/fonts/Literata-OFL.txt")),
        ("Source Sans 3 and PaperPaw Sans: SIL Open Font License 1.1",
         read("plugins/fonts-default/fonts/SourceSans3-OFL.txt")),
        ("Tabler Icons: MIT License", read("plugins/fonts-default/fonts/TablerIcons-MIT.txt")),
        ("German hyphenation patterns", german[0]),
        ("American English hyphenation patterns", english[0]),
    ]
    text = "\n\n".join(f"## {title}\n\n{paragraphs(body)}" for title, body in parts)
    where = {"firmware": "the firmware of the Xteink readers",
             "app": "the apps (Android, the desktop and web emulators)"}[product]
    for licence, crates in groups(packages).items():
        items = []
        for name, version, expression, lines, notes in crates:
            items.append(f"- {name} {version}: " + "; ".join(lines))
            items += [f"  {paragraphs(note)}" for note in notes]
        body = (f"Crates in {where} used under the {NAMES[licence]}:\n\n" + "\n".join(items)
                + "\n\n" + (
                    "Its text is in the chapter on PaperPaw's own licence above."
                    if licence == "Apache-2.0" else paragraphs(text_of(licence))))
        text += f"\n\n## Third-party crates: {NAMES[licence]}\n\n{body}"
    return text + "\n"


def outputs():
    """Every file and what it should hold."""
    packages = {product: shipped(product) for product in PRODUCTS}
    union = {}
    for found in packages.values():
        union.update(found)
    out = {"NOTICE": notice_text(union)}
    for product, found in packages.items():
        out[f"plugins/about/data/licences-{product}.md"] = app_text(product, found)
    return out


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for path, text in outputs().items():
        full = os.path.join(ROOT, path)
        if check:
            try:
                with open(full, encoding="utf-8") as f:
                    held = f.read()
            except FileNotFoundError:
                held = ""
            if held != text:
                stale.append(path)
        else:
            with open(full, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
    if stale:
        print("The notices are not what the lockfiles say (a crate added, removed or",
              "updated): run `python tools/licences/build.py` and commit:", ", ".join(stale))
        sys.exit(1)


if __name__ == "__main__":
    main()
