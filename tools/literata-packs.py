"""Builds the data plugins of Literata, which the X4's firmware leaves out
to keep room for updates: its regular, bold and italic faces
(`literata-styles`; the regular one is the file the firmware names as
`fonts-default.literata`, plugins/fonts-default), its larger sizes (36,
40 and 48 px) as packs that extend the faces, and its bold italic face
at every size, bound to the reading role. All are read from the card a
glyph at a time, while the reader has chosen Literata.

Usage:
    python tools/literata-packs.py OUT_DIR

Writes OUT_DIR/literata-styles, OUT_DIR/literata-large and
OUT_DIR/literata-bold-italic; copy a
folder into the device's plugins folder (plugins/ on the X4's card). The
release builds them so (tools/plugin-files/data.py) and offers them in
the store; their versions, names and descriptions are in
tools/plugin-files/data.toml.
"""

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools" / "plugin-files"))
import data  # noqa: E402
FONTGEN = ROOT / "target/release/paperpaw-fontgen"
FONTS = ROOT / "plugins/fonts-default/fonts"
# The sections of the plugins' `plugin.ini` that name faces; the tests of
# crates/paperpaw-app read the same files.
DATA = ROOT / "tools/plugin-files"

# The larger sizes, the sizes of `literata-styles` (the firmware's own
# font has them too, plugins/fonts-default/build.rs), and all.
LARGE = "36,40,48"
SMALL = "20,24,28,32"
ALL = "20,24,28,32,36,40,48"

# Pack of the larger sizes, font file, weight; the face each extends is
# in literata-large.ini.
FACES = [
    ("literata", "Literata.ttf", 400),
    ("literata-bold", "Literata.ttf", 700),
    ("literata-italic", "Literata-Italic.ttf", 400),
]


def pack(file, weight, sizes, out):
    subprocess.run([str(FONTGEN), str(FONTS / file), str(out), "--sizes", sizes,
                    "--chars", "latin,ligatures", "--hinting", "none",
                    "--axis", f"wght={weight}"], check=True)


def plugin(out, id, sections):
    """Its plugin.ini with `sections`, its translations and the licence,
    as data.toml names and describes it."""
    fields = data.plugins()[id]
    (out / "plugin.ini").write_text(
        data.plugin_ini(id, fields) + "formats = 3\n" + f"\n{sections}",
        encoding="utf-8",
        newline="\n",
    )
    for path, text in data.catalogs(id, fields):
        (out / path).parent.mkdir(parents=True, exist_ok=True)
        (out / path).write_text(text, encoding="utf-8", newline="\n")
    shutil.copy(FONTS / "Literata-OFL.txt", out / "fonts" / "Literata-OFL.txt")
    size = sum(f.stat().st_size for f in out.rglob("*") if f.is_file())
    print(f"{out}: {size / 1024:.0f} KiB")


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-fontgen"],
                   cwd=ROOT, check=True)
    base = Path(sys.argv[1])

    out = base / "literata-styles"
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    # The regular face: its name is in the firmware, not in plugin.ini.
    pack("Literata.ttf", 400, SMALL, out / "fonts" / "literata.ppfp")
    pack("Literata.ttf", 700, SMALL, out / "fonts" / "literata-bold.ppfp")
    pack("Literata-Italic.ttf", 400, SMALL, out / "fonts" / "literata-italic.ppfp")
    plugin(out, "literata-styles", (DATA / "literata-styles.ini").read_text(encoding="utf-8"))

    out = base / "literata-large"
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    for name, file, weight in FACES:
        pack(file, weight, LARGE, out / "fonts" / f"{name}-large.ppfp")
    plugin(out, "literata-large", (DATA / "literata-large.ini").read_text(encoding="utf-8"))

    out = base / "literata-bold-italic"
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    pack("Literata-Italic.ttf", 700, ALL, out / "fonts" / "literata-bold-italic.ppfp")
    plugin(out, "literata-bold-italic",
           "[family]\nname = Literata\nbold-italic = literata-bold-italic.ppfp\n\n"
           "[fonts]\ncore.font-reading-bold-italic = literata-bold-italic.bold-italic\n")


if __name__ == "__main__":
    main()
