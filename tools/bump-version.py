#!/usr/bin/env python3
"""Sets the release version everywhere it is written by hand (RELEASING.md,
section 1), as the beta.3 release-metadata commit did:

    python3 tools/bump-version.py 0.6.0-beta.4          # rewrite
    python3 tools/bump-version.py 0.6.0-beta.4 --check  # only list

The old version is the workspace's (`Cargo.toml`). Rewritten: the
workspace version and the internal dependency requirements, the docs and
store texts that name the release, the SDK's user agent (and its shape
files), and the Android version name and code (`0.M.P-beta.N` is
`M*10000 + P*100 + N`). Left alone on purpose: the history (CHANGELOG,
changelog.d), tests and fixtures that name a published version, and
`tools/flash-gate.sh`, whose base is the previous release.

The tracked `Cargo.lock` files are not touched here: refresh them with
`cargo metadata --format-version 1` in each workspace (RELEASING.md names
the list), then check `--locked`.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
FILES = [
    "Cargo.toml",
    "README.md",
    "CONTRIBUTING.md",
    "SECURITY.md",
    "docs/plugins.md",
    "crates/paperpaw-sdk/shape.txt",
    "crates/paperpaw-sdk/interface/paperpaw-sdk.shape",
    "crates/paperpaw-sdk/src/lib.rs",
    "crates/paperpaw-sdk/src/web.rs",
    "fastlane/metadata/android/de-DE/full_description.txt",
    "fastlane/metadata/android/en-US/full_description.txt",
    "shells/android/AndroidManifest.xml",
    "shells/android/README.md",
    "shells/xteink/README.md",
    "tools/xtensa-std/README.md",
    "tools/publish/src/repo.rs",
    "tools/dev/templates/README.md",
    "tools/dev/templates/.github/workflows/paperpaw-release.yml",
    ".claude/skills/paperpaw-plugin-development/publishing.md",
]
VERSION = re.compile(r"0\.(\d+)\.(\d+)-beta\.(\d+)")


def workspace_version():
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    found = re.search(r'^\[workspace\.package\]\n(?:.*\n)*?version = "([^"]+)"', text, re.M)
    if not found:
        sys.exit("no [workspace.package] version in Cargo.toml")
    return found.group(1)


def code(version):
    m = VERSION.fullmatch(version)
    if not m:
        sys.exit(f"{version}: not 0.M.P-beta.N")
    minor, patch, beta = map(int, m.groups())
    return minor * 10000 + patch * 100 + beta


def main(argv):
    args = [a for a in argv if not a.startswith("--")]
    check = "--check" in argv
    if len(args) != 1:
        sys.exit(__doc__)
    new = args[0]
    old = workspace_version()
    if new == old:
        sys.exit(f"the workspace is at {new} already")
    old_code, new_code = code(old), code(new)
    changed = 0
    for name in FILES:
        path = ROOT / name
        text = path.read_text(encoding="utf-8")
        out = text.replace(old, new)
        if name == "shells/android/AndroidManifest.xml":
            out = out.replace(f'versionCode="{old_code}"', f'versionCode="{new_code}"')
        if out != text:
            changed += 1
            print(f"{'would change' if check else 'changed'}: {name}")
            if not check:
                path.write_text(out, encoding="utf-8")
        else:
            print(f"UNCHANGED (names {old}?): {name}")
    print(f"{old} -> {new} (Android {old_code} -> {new_code}): {changed} of {len(FILES)} files")


if __name__ == "__main__":
    main(sys.argv[1:])
