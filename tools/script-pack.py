"""Builds a data plugin with the letters of other scripts for devices
whose built-in fonts lack them (the Xteink X4 carries Latin only): packs
read from the card a glyph at a time and asked for what the built-in
fonts lack. Cyrillic, Greek and Vietnamese come from Literata's faces and, for the
interface's font, Source Sans 3; Arabic from
Noto Naskh Arabic, Hebrew from Noto Serif Hebrew, Bengali and Devanagari
from Noto Serif Bengali and Devanagari (shaped: with their conjuncts and
the substitutions that draw them) and Ethiopic (Amharic, Tigrinya and the
other languages written in it) from Noto Serif Ethiopic, Thai from Noto Serif Thai (Noto Sans Thai for the
interface's font; tools/fonts), and Khmer, Lao and Tibetan from Noto Serif
Khmer, Lao and Tibetan (shaped as Bengali, Khmer's and Lao's packs with
their words), one pack for each built-in face they extend,
the interface's font too. The Latin letters Azerbaijani, Hausa and Yoruba add
(latin-extended) come from Literata and Source Sans 3, with what those lack
from Noto Serif and Noto Sans; Gujarati, Gurmukhi, Odia, Tamil, Telugu,
Kannada, Malayalam and Sinhala from their Noto Serif faces (shaped as
Bengali); Georgian (with its Mtavruli capitals, which
upper case gives) and Armenian from Noto Serif Georgian and Armenian.
Chinese (the 6,763 ideographs of GB 2312, or the 5,401 frequently used ones
of Big5 for Traditional) and Japanese (kana and 2,998 kanji: the Joyo and JIS X
0208 level 1) come from Noto Serif SC, TC and JP in four sizes, as Korean's.
Korean (the 2,350 Hangul syllables of KS X 1001, CJK punctuation,
compatibility jamo, full-width forms) comes from Nanum Myeongjo, one
pack for Literata's regular face and one for the interface's font (a
book's title in the library's list): the fonts lacking Hangul (bold,
italic) fall back to the first.

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

# Pack, font file, weight, the font it extends: the firmware's regular
# face, and the styles that the plugins `literata-styles` and
# `literata-bold-italic` bring from the card.
LITERATA = [
    ("literata", FONTS / "Literata.ttf", 400, "fonts-default.literata"),
    ("literata-bold", FONTS / "Literata.ttf", 700, "literata-styles.bold"),
    ("literata-italic", FONTS / "Literata-Italic.ttf", 400, "literata-styles.italic"),
    ("literata-bold-italic", FONTS / "Literata-Italic.ttf", 700, "literata-bold-italic.bold-italic"),
]


def upright(name, file):
    """Packs of a script without italics: its upright faces for all four
    of Literata's, and the regular one for the interface's font."""
    return [
        (name, file, 400, "fonts-default.literata"),
        (f"{name}-bold", file, 700, "literata-styles.bold"),
        (f"{name}-italic", file, 400, "literata-styles.italic"),
        (f"{name}-bold-italic", file, 700, "literata-bold-italic.bold-italic"),
        (f"{name}-sans", file, 400, "fonts-default.source-sans"),
    ]


def shared(name, file):
    """Packs of a script without italics, one for each weight, each
    extending every face it stands for (formats 5): the regular one
    Literata's regular and italic faces and the interface's font, the bold
    one the bold faces. Opened once for all of them, a pack's tables (an
    Indic script's substitutions) are kept in memory once."""
    return [
        (name, file, 400, ["fonts-default.literata", "literata-styles.italic",
                           "fonts-default.source-sans"]),
        (f"{name}-bold", file, 700, ["literata-styles.bold", "literata-bold-italic.bold-italic"]),
    ]


def with_sans(faces, file, name):
    """`faces` and the interface's own: the screens' texts, e.g. a book's title in
    the library's list, are drawn in Source Sans 3 (`fonts-default.source-sans`),
    which has none of the script's letters on the device, so a pack of a script
    brings them for it too, or the list shows boxes (#387)."""
    return [*faces, (name, file, 400, "fonts-default.source-sans")]


# Korean: one static face (no weight axis); its pack is not named after
# the font, whose licence reserves its name for the font itself (NOTICE).
KOREAN = [
    ("korean", OTHER_FONTS / "NanumMyeongjo-Regular.ttf", None, "fonts-default.literata"),
    ("korean-sans", OTHER_FONTS / "NanumMyeongjo-Regular.ttf", None, "fonts-default.source-sans"),
]
# Thai: Noto Serif Thai for Literata's four faces (its weights 400 and 700, upright
# for all four), Noto Sans Thai for the interface's font (the screens' texts).
THAI = [*upright("thai", OTHER_FONTS / "NotoSerifThai.ttf")[:4],
        ("thai-sans", OTHER_FONTS / "NotoSansThai.ttf", 400, "fonts-default.source-sans")]
# Chinese and Japanese: Noto Serif (SIL OFL 1.1; tools/fonts holds the characters the packs
# need, subset from the font, static at weight 400), one pack for Literata's regular face and
# one for the interface's font, as Korean's. Sizes 20 to 32 px (what the firmware's fonts
# have; 36 to 48 px of `literata-large` are drawn at 32): at six thousand characters each
# size is megabytes.
CJK_SIZES = "20,24,28,32"


CJK = {"chinese-simplified", "chinese-traditional", "japanese"}


def han(name, file):
    return [(name, file, None, "fonts-default.literata"),
            (f"{name}-sans", file, None, "fonts-default.source-sans")]


# Latin Extended (#385): Azerbaijani's schwa, Yoruba's letters with a dot below and the
# combining tone marks, from Literata's faces and Source Sans 3; what those lack (Hausa's
# hooked letters, n and m with a grave or acute) from Noto Serif, a pack for each weight
# shared by the faces it stands for (as `shared`), and Noto Sans for the interface's font.
LATIN_OWN = ["U+018F", "U+0259", "U+1E62-1E63", "U+1EB8-1EB9", "U+1ECC-1ECD",
             "U+0300-0304", "U+030C", "U+0323"]
LATIN_NOTO = "U+0181,U+0253,U+018A,U+0257,U+0198-0199,U+01B3-01B4,U+01F8-01F9,U+1E3E-1E3F"
LATIN_EXTENDED = [
    *with_sans(LITERATA, FONTS / "SourceSans3.ttf", "latin-extended-sans"),
    ("noto-serif", OTHER_FONTS / "NotoSerif-subset.ttf", 400,
     ["fonts-default.literata", "literata-styles.italic"], LATIN_NOTO),
    ("noto-serif-bold", OTHER_FONTS / "NotoSerif-subset.ttf", 700,
     ["literata-styles.bold", "literata-bold-italic.bold-italic"], LATIN_NOTO),
    ("noto-sans", OTHER_FONTS / "NotoSans-subset.ttf", 400, "fonts-default.source-sans", LATIN_NOTO),
]

# The other 8,822 syllables, a pack of its own that only adds letters to
# the Korean pack (about 30 MiB on the card): offered for books that use them.
KOREAN_EXTRA = [("korean-extra", OTHER_FONTS / "NanumMyeongjo-Regular.ttf", None,
                 "fonts-default.literata")]

# Per set: its name, the faces it is drawn in, the font's licence, the
# font named in the plugin's description, and the characters
# (paperpaw-fontgen's --chars) where they are not the set of that name. A
# face may name characters of its own, fifth (a font for what the others lack).
SETS = {
    "latin-extended": ("Latin Extended", LATIN_EXTENDED,
                       [FONTS / "Literata-OFL.txt", FONTS / "SourceSans3-OFL.txt",
                        OTHER_FONTS / "NotoSerif-OFL.txt", OTHER_FONTS / "NotoSans-OFL.txt"],
                       "Literata", LATIN_OWN),
    # Georgian and Armenian (#385): alphabets, no shaping; upright, a pack for each weight.
    # Georgian's Mtavruli: what upper case (a word clock's grid, "In capitals") makes of it.
    "georgian": ("Georgian", shared("georgian", OTHER_FONTS / "NotoSerifGeorgian.ttf"),
                 OTHER_FONTS / "NotoSerifGeorgian-OFL.txt", "Noto Serif Georgian",
                 ["U+10D0-10FF", "U+1C90-1CBF"]),
    "armenian": ("Armenian", shared("armenian", OTHER_FONTS / "NotoSerifArmenian.ttf"),
                 OTHER_FONTS / "NotoSerifArmenian-OFL.txt", "Noto Serif Armenian", ["U+0531-058F"]),
    # Source Sans 3 has these letters: the interface's face keeps its look.
    "cyrillic": ("Cyrillic", with_sans(LITERATA, FONTS / "SourceSans3.ttf", "cyrillic-sans"),
                 [FONTS / "Literata-OFL.txt", FONTS / "SourceSans3-OFL.txt"], "Literata", None),
    "greek": ("Greek", with_sans(LITERATA, FONTS / "SourceSans3.ttf", "greek-sans"),
              [FONTS / "Literata-OFL.txt", FONTS / "SourceSans3-OFL.txt"], "Literata", None),
    "vietnamese": ("Vietnamese", with_sans(LITERATA, FONTS / "SourceSans3.ttf", "vietnamese-sans"),
                   [FONTS / "Literata-OFL.txt", FONTS / "SourceSans3-OFL.txt"], "Literata", None),
    "arabic": ("Arabic", upright("naskh", OTHER_FONTS / "NotoNaskhArabic.ttf"),
               OTHER_FONTS / "NotoNaskhArabic-OFL.txt", "Noto Naskh Arabic", None),
    "hebrew": ("Hebrew", upright("hebrew", OTHER_FONTS / "NotoSerifHebrew.ttf"),
               OTHER_FONTS / "NotoSerifHebrew-OFL.txt", "Noto Serif Hebrew", None),
    "thai": ("Thai", THAI, [OTHER_FONTS / "NotoSerifThai-OFL.txt", OTHER_FONTS / "NotoSansThai-OFL.txt"],
             "Noto Serif Thai", ["U+0E01-0E3A", "U+0E3F-0E5B"]),
    # The syllabary with its punctuation and numerals (U+1200 to U+137F) and the
    # blocks the font has for the other languages written in it: Supplement,
    # Extended and Extended-A. (Extended-B, U+1E7E0 to U+1E7FF, is left out.)
    # No italics: a pack for each weight, shared by the faces it stands for.
    "ethiopic": ("Ethiopic", shared("ethiopic", OTHER_FONTS / "NotoSerifEthiopic.ttf"),
                 OTHER_FONTS / "NotoSerifEthiopic-OFL.txt", "Noto Serif Ethiopic",
                 ["U+1200-139F", "U+2D80-2DDF", "U+AB00-AB2F"]),
    "korean": ("Korean", KOREAN, OTHER_FONTS / "NanumMyeongjo-OFL.txt", "a Korean book face",
               [f"@{ROOT / 'tools/fontgen/sets/hangul-ks.txt'}",
                "cjk-punctuation", "hangul-jamo", "fullwidth"]),
    "chinese-simplified": ("Chinese (Simplified)", han("chinese-simplified", OTHER_FONTS / "NotoSerifSC-subset.ttf"),
                           OTHER_FONTS / "NotoSerifCJK-OFL.txt", "Noto Serif SC",
                           [f"@{ROOT / 'tools/fontgen/sets/hanzi-gb2312.txt'}",
                            "cjk-punctuation", "fullwidth"]),
    "chinese-traditional": ("Chinese (Traditional)", han("chinese-traditional", OTHER_FONTS / "NotoSerifTC-subset.ttf"),
                            OTHER_FONTS / "NotoSerifCJK-OFL.txt", "Noto Serif TC",
                            [f"@{ROOT / 'tools/fontgen/sets/hanzi-big5.txt'}",
                             "cjk-punctuation", "fullwidth"]),
    "japanese": ("Japanese", han("japanese", OTHER_FONTS / "NotoSerifJP-subset.ttf"),
                 OTHER_FONTS / "NotoSerifCJK-OFL.txt", "Noto Serif JP",
                 [f"@{ROOT / 'tools/fontgen/sets/kanji-japanese.txt'}",
                  "kana", "cjk-punctuation", "fullwidth"]),
    # Indic (#157): the fonts' conjuncts and other forms go into the packs
    # with the substitutions that draw them (paperpaw_text's indic module).
    "bengali": ("Bengali", shared("bengali", OTHER_FONTS / "NotoSerifBengali.ttf"),
                OTHER_FONTS / "NotoSerifBengali-OFL.txt", "Noto Serif Bengali", None),
    "devanagari": ("Devanagari", shared("devanagari", OTHER_FONTS / "NotoSerifDevanagari.ttf"),
                   OTHER_FONTS / "NotoSerifDevanagari-OFL.txt", "Noto Serif Devanagari", None),
    # Khmer, Lao, Tibetan (#314): shaped as Bengali (Tibetan's clusters kept
    # shaped, paperpaw_text's indic module); Khmer's and Lao's packs carry
    # their words (tools/word-table.py, from ICU's lists).
    "khmer": ("Khmer", shared("khmer", OTHER_FONTS / "NotoSerifKhmer.ttf"),
              [OTHER_FONTS / "NotoSerifKhmer-OFL.txt", OTHER_FONTS / "ICU-word-lists-LICENSE.txt"],
              "Noto Serif Khmer", ["U+1780-17FF"]),
    "lao": ("Lao", shared("lao", OTHER_FONTS / "NotoSerifLao.ttf"),
            [OTHER_FONTS / "NotoSerifLao-OFL.txt", OTHER_FONTS / "ICU-word-lists-LICENSE.txt"],
            "Noto Serif Lao", ["U+0E80-0EFF"]),
    "tibetan": ("Tibetan", shared("tibetan", OTHER_FONTS / "NotoSerifTibetan.ttf"),
                OTHER_FONTS / "NotoSerifTibetan-OFL.txt", "Noto Serif Tibetan", ["U+0F00-0FFF"]),
    # Gujarati, Gurmukhi, Oriya (Odia), Tamil, Telugu, Kannada, Malayalam, Sinhala (#385):
    # shaped as Bengali (paperpaw_text's indic module), with the dandas they share (and
    # Sinhala's joiners, which its fonts draw its touching letters by).
    "gujarati": ("Gujarati", shared("gujarati", OTHER_FONTS / "NotoSerifGujarati.ttf"),
                 OTHER_FONTS / "NotoSerifGujarati-OFL.txt", "Noto Serif Gujarati", ["U+0A80-0AFF"]),
    "gurmukhi": ("Gurmukhi", shared("gurmukhi", OTHER_FONTS / "NotoSerifGurmukhi.ttf"),
                 OTHER_FONTS / "NotoSerifGurmukhi-OFL.txt", "Noto Serif Gurmukhi", ["U+0A00-0A7F", "U+0964-0965"]),
    "oriya": ("Odia", shared("oriya", OTHER_FONTS / "NotoSerifOriya.ttf"),
              OTHER_FONTS / "NotoSerifOriya-OFL.txt", "Noto Serif Oriya", ["U+0B00-0B7F", "U+0964-0965"]),
    "tamil": ("Tamil", shared("tamil", OTHER_FONTS / "NotoSerifTamil.ttf"),
              OTHER_FONTS / "NotoSerifTamil-OFL.txt", "Noto Serif Tamil", ["U+0B80-0BFF"]),
    "telugu": ("Telugu", shared("telugu", OTHER_FONTS / "NotoSerifTelugu.ttf"),
               OTHER_FONTS / "NotoSerifTelugu-OFL.txt", "Noto Serif Telugu", ["U+0C00-0C7F"]),
    "kannada": ("Kannada", shared("kannada", OTHER_FONTS / "NotoSerifKannada.ttf"),
                OTHER_FONTS / "NotoSerifKannada-OFL.txt", "Noto Serif Kannada", ["U+0C80-0CFF"]),
    "malayalam": ("Malayalam", shared("malayalam", OTHER_FONTS / "NotoSerifMalayalam.ttf"),
                  OTHER_FONTS / "NotoSerifMalayalam-OFL.txt", "Noto Serif Malayalam", ["U+0D00-0D7F"]),
    "sinhala": ("Sinhala", shared("sinhala", OTHER_FONTS / "NotoSerifSinhala.ttf"),
                OTHER_FONTS / "NotoSerifSinhala-OFL.txt", "Noto Serif Sinhala", ["U+0D80-0DFF", "U+200C-200D"]),
    "korean-extra": ("Korean", KOREAN_EXTRA, OTHER_FONTS / "NanumMyeongjo-OFL.txt",
                     "a Korean book face", [f"@{ROOT / 'tools/fontgen/sets/hangul-rest.txt'}"]),
}


# Keyboard layouts a pack brings (tools/plugin-files/keyboards/<set>.txt, the
# lines of the keyboard's point `keyboard.layouts`), and their names.
NAMES = {
    "keyboard.layout.ar": ("Arabic", "Arabisch"),
    "keyboard.layout.fa": ("Persian", "Persisch"),
    "keyboard.layout.ur": ("Urdu", "Urdu"),
    "keyboard.layout.he": ("Hebrew", "Hebräisch"),
    "keyboard.layout.ko": ("Korean", "Koreanisch"),
}


def layouts(out, sets):
    """Writes the layouts of `sets` as `points/keyboard.layouts.txt`, and
    their names to the plugin's translations."""
    lines = []
    for name in sets:
        file = ROOT / "tools/plugin-files/keyboards" / f"{name}.txt"
        if file.exists():
            lines += [l for l in file.read_text(encoding="utf-8").splitlines() if l.strip()]
    if not lines:
        return
    (out / "points").mkdir(parents=True, exist_ok=True)
    (out / "points/keyboard.layouts.txt").write_text("\n".join(lines) + "\n", encoding="utf-8",
                                                    newline="\n")
    ids = [l.split(";")[0].strip() for l in lines if not l.startswith("#")]
    for n, lang in enumerate(["en", "de"]):
        path = out / "i18n" / f"{lang}.ini"
        names = "".join(f"{i} = {NAMES[i][n]}\n" for i in ids)
        # A pack not in data.toml (yet) has no translations of its own.
        text = (path.read_text(encoding="utf-8") if path.exists()
                else f"[meta]\nlanguage = {lang}\n\n[messages]\n")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text + names, encoding="utf-8", newline="\n")


def build(base, sets):
    """Writes the plugin of `sets` to `base`/script-<sets>."""
    faces, licence, font, _ = SETS[sets[0]][1:]
    if any(SETS[s][1] != faces for s in sets):
        sys.exit("sets drawn from other fonts go in plugins of their own")
    chars = ",".join(c for s in sets for c in (SETS[s][4] or [s]))
    id = "script-" + "-".join(sets)
    out = base / id
    shutil.rmtree(out, ignore_errors=True)
    (out / "fonts").mkdir(parents=True)
    sizes = ["--sizes", CJK_SIZES] if sets[0] in CJK else []
    for name, file, weight, _, *own in faces:
        axis = ["--axis", f"wght={weight}"] if weight else []
        subprocess.run([str(FONTGEN), str(file), str(out / "fonts" / f"{name}.ppfp"),
                        "--chars", own[0] if own else chars, "--hinting", "none", *axis, *sizes], check=True)
    # Thai's words, for breaking lines between them (#313): in the packs of
    # Literata's regular face and of the interface's font, one of which is
    # open wherever Thai is set.
    if "thai" in sets:
        subprocess.run([sys.executable, str(ROOT / "tools/thai-dictionary.py"), "add",
                        str(out / "fonts/thai.ppfp"), str(out / "fonts/thai-sans.ppfp")], check=True)
    # Khmer's and Lao's (#314): in both of their packs.
    for script in [s for s in sets if s in ("khmer", "lao")]:
        subprocess.run([sys.executable, str(ROOT / "tools/word-table.py"), "add", script,
                        *(str(out / "fonts" / f"{name}.ppfp") for name, *_ in faces)], check=True)
    extends = "\n".join(
        f"{name}.ppfp = {', '.join(extended) if isinstance(extended, list) else extended}"
        for name, _, _, extended, *_ in faces)
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
    layouts(out, sets)
    (out / "plugin.ini").write_text(f"{head}\n[extends]\n{extends}\n", encoding="utf-8",
                                    newline="\n")
    for file in licence if isinstance(licence, list) else [licence]:
        shutil.copy(file, out / "fonts" / file.name)
    size = sum(f.stat().st_size for f in out.rglob("*") if f.is_file())
    print(f"{out}: {size / 1024 / 1024:.1f} MiB")


def main():
    if len(sys.argv) < 2 or any(a.startswith("-") for a in sys.argv[1:]):
        sys.exit(__doc__)
    sets = sys.argv[2:]
    unknown = [s for s in sets if s not in SETS]
    if unknown:
        sys.exit(f"unknown character sets: {', '.join(unknown)}")
    if sets:
        plugins = [sets]
    else:
        # A pack named for a set of its own (script-korean-extra), else for
        # the sets it holds (script-cyrillic-greek).
        plugins = [[named] if (named := id.removeprefix("script-")) in SETS else named.split("-")
                   for id, fields in data.plugins().items()
                   if fields["build"] == "tools/script-pack.py"] or [["cyrillic", "greek"]]
    subprocess.run(["cargo", "build", "-q", "--release", "-p", "paperpaw-fontgen"],
                   cwd=ROOT, check=True)
    for sets in plugins:
        build(Path(sys.argv[1]), sets)


if __name__ == "__main__":
    main()
