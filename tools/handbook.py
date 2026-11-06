#!/usr/bin/env python3
"""The handbook of a release: what tools that write or check plugins for
it read (the workshop's AI handbook, its lookups and checks), so they need
no PaperPaw checkout of their own:

    python3 tools/handbook.py <registry.json> <version> <out.zip>

The zip holds, under `paperpaw-handbook-<version>/`:

- README.md: this list, the version, the format (`FORMAT`);
- registry.json: what the app offers data plugins (`paperpaw-emu
  --registry`): settings, widgets, actions, device profiles;
- checks.json: every finding paperpaw-dev's checks make, by its code
  (`visual.wrap`), each with its group (`visual`);
- docs/: the repository's docs (the plugin guide `docs/plugins.md`, the
  ADRs);
- samples/: the sample plugins, data and code;
- api/<crate>.txt: the public items of the SDK and the crates it passes
  on, as their `api.txt` lists them;
- src/<crate>/: the source of those and of every crate the SDK is built
  from, for the items' documentation;
- plugins/<id>/: PaperPaw's own plugins (their `Cargo.toml`, `src/`,
  `i18n/`, `how/`, `plugin.ini`), built against the same SDK: examples of
  how a plugin solves things, to search where one is wanted.

Never in it: tests (also those in `src/`), pictures, fonts and the sample
books, design proposals still in draft (`docs/abi/proposals/`), the
generated table of icon names (`icon_table.rs`), what a build or a run
leaves, and the kernel and the shells, which a plugin never uses. A field or file is only added; FORMAT counts up
where one changes.
"""

import json
import re
import subprocess
import sys
import zipfile
from pathlib import Path

FORMAT = 1
ROOT = Path(__file__).resolve().parent.parent
# The groups of paperpaw-dev's checks (its commands), the start of a code.
GROUPS = ("package", "lint", "visual", "reach", "explore", "perf", "flows",
          "gallery", "snapshots", "ui-kit", "data")
CODE = re.compile(r'"((?:%s)\.[a-z0-9-]+(?:\.[a-z0-9-]+)*)"' % "|".join(GROUPS))
# Below a crate's folder, what is not part of it.
SKIP = {"target", ".git", "node_modules", "out", "tests", "testdata", "snapshots"}
# Large files a model reads nothing in: pictures, fonts, books, builds.
ASSETS = {".png", ".jpg", ".jpeg", ".gif", ".webp", ".ttf", ".otf", ".ttc", ".ppfp",
          ".epub", ".fb2", ".pdf", ".bin", ".ppn"}
# Folders of such files, by their path.
ASSET_FOLDERS = ("samples/library/books/",)
# Design proposals still in draft: working papers for the team, replaced by
# the ADR and the docs they become, not what a plugin author reads.
DRAFT_FOLDERS = ("docs/abi/proposals/",)
# Generated tables of data with nothing to read in them: the names of all
# of Tabler's icons in one string (172 KB, 53 KB zipped; `tools/icons/subset.py`
# makes it). The names are public and `icons.rs` says how they are used.
GENERATED_FOLDERS = ("crates/paperpaw-sdk/src/icon_table.rs",)
# What of a plugin's folder the handbook holds.
PLUGIN_PARTS = ("Cargo.toml", "plugin.ini", "src", "i18n", "how")


def check_codes(dev: Path) -> list:
    """Every code a finding of paperpaw-dev can have, from its source."""
    codes = set()
    for path in dev.rglob("*.rs"):
        if "tests" in path.relative_to(dev).parts:
            continue
        codes.update(CODE.findall(path.read_text(encoding="utf-8")))
    return [{"code": code, "group": code.split(".")[0]} for code in sorted(codes)]


def files(folder: Path):
    """The files of `folder` that git keeps (never a build, nor what a
    local run left: a library's `.paperpaw` state, keys), without
    dot-folders."""
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", folder.relative_to(ROOT).as_posix()],
        cwd=ROOT, check=True, capture_output=True,
    ).stdout.decode("utf-8")
    for name in sorted(filter(None, listed.split("\0"))):
        path = ROOT / name
        parts = path.relative_to(folder).parts
        test = path.name == "tests.rs" or path.name.endswith("_tests.rs")
        if (path.is_file() and not any(p.startswith(".") for p in parts)
                and not SKIP.intersection(parts) and not test
                and path.suffix.lower() not in ASSETS
                and not name.startswith(ASSET_FOLDERS + DRAFT_FOLDERS + GENERATED_FOLDERS)):
            yield path


def without_tests(text: str) -> str:
    """A Rust file without the module of tests at its end
    (`#[cfg(test)] mod tests { ... }`); as it is where that is not its
    last item. As rustfmt writes it, the module ends at the first line
    that is `}` alone (braces in its strings never count)."""
    start = text.find("\n#[cfg(test)]\nmod tests {\n")
    if start < 0:
        return text
    end = text.find("\n}\n", start)
    # After it, at most more tests in files of their own (`mod x_tests;`).
    rest = re.sub(r"#\[cfg\(test\)\]\s*mod \w+;", "", text[end + 3:]) if end >= 0 else "x"
    if rest.strip():
        return text
    return text[:start + 1]


def plugin_files(plugins: Path):
    """The files of PaperPaw's own plugins the handbook holds, by plugin:
    their manifest, source, translations and How it works."""
    for path in files(plugins):
        if path.relative_to(plugins).parts[1] in PLUGIN_PARTS:
            yield path


def crates_with_api() -> list:
    """The crates with a list of their public items: the SDK and those it
    passes on."""
    return sorted(p.parent for p in (ROOT / "crates").glob("*/api.txt"))


def sdk_crates() -> list:
    """The SDK and every PaperPaw crate it is built from (`Canvas`, `Rect`
    and `Font` are theirs), whose source documents what a plugin uses."""
    manifest = (ROOT / "crates" / "paperpaw-sdk" / "Cargo.toml").read_text(encoding="utf-8")
    names = set(re.findall(r"^(paperpaw-[a-z-]+)\s*[.=]", manifest, re.M)) | {"paperpaw-sdk"}
    return sorted(ROOT / "crates" / n for n in names if (ROOT / "crates" / n / "src").is_dir())


def main(registry: str, version: str, out: str) -> None:
    top = f"paperpaw-handbook-{version}"
    codes = check_codes(ROOT / "tools" / "dev" / "src")
    if len(codes) < 20:
        sys.exit(f"only {len(codes)} check codes found: has paperpaw-dev's source moved?")
    crates = crates_with_api()
    if not any(c.name == "paperpaw-sdk" for c in crates):
        sys.exit("no crates/paperpaw-sdk/api.txt")
    plugins = len({p.relative_to(ROOT / "plugins").parts[0] for p in plugin_files(ROOT / "plugins")})
    if plugins < 10:
        sys.exit(f"only {plugins} plugins found: has plugins/ moved?")
    readme = f"""# PaperPaw {version}: the handbook

Format {FORMAT}. What tools that write or check plugins for PaperPaw
{version} read, made by its release (tools/handbook.py):

- registry.json: settings, widgets, actions, device profiles (`paperpaw-emu --registry`)
- checks.json: the codes of paperpaw-dev's findings ({len(codes)})
- docs/: the docs, the plugin guide `docs/plugins.md` first
- samples/: the sample plugins
- api/: the public items of {", ".join(c.name for c in crates)}
- src/: the source of those and of the crates the SDK is built from
- plugins/: PaperPaw's own plugins ({plugins}): source, translations, How it works

What each part is for:

- The SDK (api/, src/) is what a plugin may use, and all it may use:
  `paperpaw-sdk` and the crates it passes on. A plugin builds against it
  alone.
- PaperPaw's own plugins (plugins/) are examples of good solutions,
  built against the same SDK: how a screen, a setting, a widget, a sync
  or a reader feature is done. Search them when a plugin needs something
  one of them does; they are not an API, and a plugin does not depend on
  them.
- The kernel and the shells (the devices' programs) are left out on
  purpose: a plugin never uses them, only the SDK.
- docs/ explains how plugins are written (`docs/plugins.md`) and why
  PaperPaw is as it is (docs/adr); samples/ are small plugins to start
  from; registry.json and checks.json are what the app offers data
  plugins and what paperpaw-dev checks.

Tests, pictures, fonts, the sample books and design proposals still in
draft are not in it.
"""
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr(f"{top}/README.md", readme)
        # PaperPaw's licence and the notices of what it holds.
        for name in ("LICENSE", "NOTICE"):
            z.write(ROOT / name, f"{top}/{name}")
        z.write(registry, f"{top}/registry.json")
        z.writestr(f"{top}/checks.json", json.dumps({"format": FORMAT, "checks": codes}, indent=1) + "\n")
        for folder in ("docs", "samples"):
            for path in files(ROOT / folder):
                z.write(path, f"{top}/{path.relative_to(ROOT).as_posix()}")
        for crate in crates:
            z.write(crate / "api.txt", f"{top}/api/{crate.name}.txt")
        def source(path: Path, name: str) -> None:
            if path.suffix == ".rs":
                z.writestr(name, without_tests(path.read_text(encoding="utf-8")))
            else:
                z.write(path, name)

        for crate in sorted(set(crates) | set(sdk_crates())):
            for path in files(crate / "src"):
                source(path, f"{top}/src/{crate.name}/{path.relative_to(crate / 'src').as_posix()}")
        for path in plugin_files(ROOT / "plugins"):
            source(path, f"{top}/{path.relative_to(ROOT).as_posix()}")
    size = Path(out).stat().st_size
    print(f"{out}: {len(codes)} check codes, {len(crates)} crates, {plugins} plugins, "
          f"{size / 1e6:.1f} MB")


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__.split("\n\n")[1])
    main(*sys.argv[1:])
