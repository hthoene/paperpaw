"""Builds a data plugin with the letters of other scripts for devices
whose built-in fonts lack them (the Xteink X4 carries Latin only): packs
read from the card a glyph at a time and asked for what the built-in
fonts lack. Cyrillic and Greek come from Literata's faces; Arabic from
Noto Naskh Arabic and Hebrew from Noto Serif Hebrew (tools/fonts), one
pack for each built-in face they extend, the interface's font too.

Usage:
    python tools/script-pack.py OUT_DIR [SET ...]

The plugin of the SETs (see paperpaw_text::charset::named; sets drawn
from one font only go together, arabic and hebrew alone) is written to
OUT_DIR/script-<sets>; copy that folder into the device's plugins folder
(e.g. plugins/ on the X4's card). Without a SET, those
tools/plugin-files/data.toml lists for the store are built, as the
release builds them (data.py), else Cyrillic and Greek. A plugin
data.toml lists takes its version, name and description (English and
German) from there.
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


def build(base, sets):
    """Writes the plugin of `sets` to `base`/script-<sets>."""
    faces, licence, font = SETS[sets[0]][1:]
    if any(SETS[s][1] != faces for s in sets):
        sys.exit("sets drawn from other fonts go in plugins of their own")
    id = "script-" + "-".join(sets)
    out = base / id
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    for name, file, weight, _ in faces:
        subprocess.run([str(FONTGEN), str(file), str(out / "fonts" / f"{name}.ppfp"),
                        "--chars", ",".join(sets), "--axis", f"wght={weight}"], check=True)
    extends = "\n".join(f"{name}.ppfp = {extended}" for name, _, _, extended in faces)
    listed = data.plugins().get(id)
    if listed:
        head = data.plugin_ini(id, listed)
        for path, text in data.catalogs(id, listed):
            (out / path).parent.mkdir(parents=True, exist_ok=True)
            (out / path).write_text(text, encoding="utf-8", newline="\n")
    else:
        title = " and ".join(SETS[s][0] for s in sets)
        head = ("[plugin]\n"
                f"id = {id}\n"
                f"name = {title} letters\n"
                f"description = {title} letters in {font}, for devices whose fonts lack them.\n"
                "version = 0.1.0\n")
    (out / "plugin.ini").write_text(f"{head}\n[extends]\n{extends}\n", encoding="utf-8",
                                    newline="\n")
    shutil.copy(licence, out / "fonts" / licence.name)
    size = sum(f.stat().st_size for f in out.rglob("*") if f.is_file())
    print(f"{out}: {size / 1024 / 1024:.1f} MiB")


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    sets = sys.argv[2:]
    unknown = [s for s in sets if s not in SETS]
    if unknown:
        sys.exit(f"unknown character sets: {', '.join(unknown)}")
    if sets:
        plugins = [sets]
    else:
        plugins = [id.removeprefix("script-").split("-")
                   for id, fields in data.plugins().items()
                   if fields["build"] == "tools/script-pack.py"] or [["cyrillic", "greek"]]
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-fontgen"],
                   cwd=ROOT, check=True)
    for sets in plugins:
        build(Path(sys.argv[1]), sets)


if __name__ == "__main__":
    main()
