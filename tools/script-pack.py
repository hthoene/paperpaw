"""Builds a data plugin with the letters of other scripts for devices
whose built-in fonts lack them (the Xteink X4 carries Latin only): packs
read from the card a glyph at a time and asked for what the built-in
fonts lack. Cyrillic and Greek come from Literata's faces; Arabic from
Noto Naskh Arabic and Hebrew from Noto Serif Hebrew (tools/fonts), one
pack for each built-in face they extend, the interface's font too.

Usage:
    python tools/script-pack.py OUT_DIR [SET ...]

SET defaults to cyrillic and greek (see paperpaw_text::charset::named);
sets drawn from one font only go together (arabic, hebrew alone). The
plugin is written to OUT_DIR/<id>; copy that folder into the device's
plugins folder (e.g. plugins/ on the X4's card).
"""

import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FONTGEN = ROOT / "target/release/paperpaw-fontgen"
FONTS = ROOT / "plugins/fonts-default/fonts"
OTHER_FONTS = ROOT / "tools/fonts"

# Pack, font file, weight, the built-in font it extends.
LITERATA = [
    ("literata", FONTS / "Literata.ttf", 400, "fonts-default.literata"),
    ("literata-bold", FONTS / "Literata.ttf", 700, "fonts-default.literata-bold"),
    ("literata-italic", FONTS / "Literata-Italic.ttf", 400, "fonts-default.literata-italic"),
    ("literata-bold-italic", FONTS / "Literata-Italic.ttf", 700, "fonts-default.literata-bold-italic"),
]


def upright(name, file):
    """Packs of a script without italics: its upright faces for all four
    of Literata's, and the regular one for the interface's font."""
    return [
        (name, file, 400, "fonts-default.literata"),
        (f"{name}-bold", file, 700, "fonts-default.literata-bold"),
        (f"{name}-italic", file, 400, "fonts-default.literata-italic"),
        (f"{name}-bold-italic", file, 700, "fonts-default.literata-bold-italic"),
        (f"{name}-sans", file, 400, "fonts-default.source-sans"),
    ]


# Per set: its name, the faces it is drawn in, the font's licence, and
# the font named in the plugin's description.
SETS = {
    "cyrillic": ("Cyrillic", LITERATA, FONTS / "Literata-OFL.txt", "Literata"),
    "greek": ("Greek", LITERATA, FONTS / "Literata-OFL.txt", "Literata"),
    "arabic": ("Arabic", upright("naskh", OTHER_FONTS / "NotoNaskhArabic.ttf"),
               OTHER_FONTS / "NotoNaskhArabic-OFL.txt", "Noto Naskh Arabic"),
    "hebrew": ("Hebrew", upright("hebrew", OTHER_FONTS / "NotoSerifHebrew.ttf"),
               OTHER_FONTS / "NotoSerifHebrew-OFL.txt", "Noto Serif Hebrew"),
}


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    sets = sys.argv[2:] or ["cyrillic", "greek"]
    unknown = [s for s in sets if s not in SETS]
    if unknown:
        sys.exit(f"unknown character sets: {', '.join(unknown)}")
    faces, licence, font = SETS[sets[0]][1:]
    if any(SETS[s][1] != faces for s in sets):
        sys.exit("sets drawn from other fonts go in plugins of their own")
    id = "script-" + "-".join(sets)
    out = Path(sys.argv[1]) / id
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-fontgen"],
                   cwd=ROOT, check=True)
    for name, file, weight, _ in faces:
        subprocess.run([str(FONTGEN), str(file), str(out / "fonts" / f"{name}.ppfp"),
                        "--chars", ",".join(sets), "--axis", f"wght={weight}"], check=True)
    title = " and ".join(SETS[s][0] for s in sets)
    extends = "\n".join(f"{name}.ppfp = {extended}" for name, _, _, extended in faces)
    (out / "plugin.ini").write_text(
        "[plugin]\n"
        f"id = {id}\n"
        f"name = {title} letters\n"
        f"description = {title} letters in {font}, for devices whose fonts lack them.\n"
        "version = 0.1.0\n"
        "\n[extends]\n"
        f"{extends}\n",
        encoding="utf-8",
    )
    shutil.copy(licence, out / "fonts" / licence.name)
    size = sum(f.stat().st_size for f in out.rglob("*") if f.is_file())
    print(f"{out}: {size / 1024 / 1024:.1f} MiB")


if __name__ == "__main__":
    main()
