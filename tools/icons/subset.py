"""Builds PaperPaw's icon font: the icons it uses, taken from Tabler Icons.

Usage:
    python tools/icons/subset.py path/to/tabler-icons.ttf

The font comes from the npm package `@tabler/icons-webfont` (MIT licence,
see plugins/fonts-default/fonts/TablerIcons-MIT.txt), file
`dist/fonts/tabler-icons.ttf`, version TABLER below. Needs fontTools.

Writes:
- plugins/fonts-default/fonts/PaperPawIcons.ttf: only the icons in ICONS,
  at Tabler's code points (private use area);
- crates/paperpaw-sdk/src/icon_table.rs: the code points of all of
  Tabler's icons by the hashes of their names, for `paperpaw_sdk::icons`
  (plugins name any; icon packs bring the drawings the font lacks, see
  pack.py), and the names themselves for `icons::names` (tools and tests).

To add an icon, add its Tabler name to ICONS (https://tabler.io/icons)
and run this again.
"""

import sys
from pathlib import Path

from fontTools import subset
from fontTools.ttLib import TTFont

TABLER = "3.35.0"

# Icons by their Tabler names; PaperPaw uses the same names.
ICONS = sorted({
    # Places
    "home", "books", "book", "book-2", "bookmark", "bookmarks", "list",
    "list-details", "folder", "folder-open", "file", "file-text", "files",
    "wifi-off", "inbox",
    # Reading
    "search", "typography", "text-size", "letter-case", "adjustments",
    "adjustments-horizontal", "layout", "layout-bottombar", "layout-grid",
    "columns", "highlight", "cursor-text", "clock-play", "arrows-horizontal",
    "pencil",
    "note", "hash", "arrow-bar-to-right", "language", "a-b", "link",
    "clipboard", "copy", "selector", "volume", "tag",
    # Light and display
    "sun", "sun-low", "moon", "temperature", "brightness", "contrast",
    "refresh",
    # Tools
    "chart-bar", "calendar", "clock", "wifi", "bluetooth", "world", "cloud",
    "cloud-download", "download", "upload", "rss", "qrcode", "keyboard",
    "plug", "puzzle", "vocabulary", "transfer", "apps", "camera",
    "book-download", "article", "devices", "device-desktop", "stack-2",
    "automation", "circle-arrow-up", "list-check",
    # System
    "settings", "menu-2", "dots", "dots-vertical", "info-circle", "power",
    "battery", "device-tablet", "user", "lock", "palette", "bug",
    "hourglass", "hand-click", "hand-finger", "wand", "key", "eraser",
    "sparkles", "bulb", "paw", "leaf",
    # Controls
    "chevron-left", "chevron-right", "chevron-up", "chevron-down",
    "arrow-left", "arrow-right", "arrow-up", "arrow-down", "x", "check", "plus",
    "minus", "trash",
    "alert-triangle", "edit", "star", "eye", "sort-ascending", "filter", "circle",
    "circle-check", "toggle-left", "toggle-right",
})


def tabler_codes(path):
    """Every icon of Tabler's font at `path`: its name and code point."""
    return {name: code for code, name in TTFont(path).getBestCmap().items()}


def icon_font(path, codes):
    """Tabler's font at `path` with only the icons at `codes`, its metrics
    set from the drawings."""
    options = subset.Options()
    options.layout_features = []
    options.name_IDs = ["*"]
    options.glyph_names = True
    options.notdef_outline = True
    subsetter = subset.Subsetter(options)
    font = TTFont(path)
    subsetter.populate(unicodes=codes)
    subsetter.subset(font)
    # Tabler's metrics put every left side bearing at 0 although the
    # drawings start further right; renderers that follow the metrics
    # (as PaperPaw's does) would draw icons off centre. The bearings are
    # set from the drawings, and the ascent and descent to the drawings'
    # extent, so an icon centred by its line box is centred by its ink.
    glyf, hmtx = font["glyf"], font["hmtx"]
    top, bottom = 0, 0
    for name in font.getGlyphOrder():
        glyph = glyf[name]
        if glyph.numberOfContours == 0:
            continue
        glyph.recalcBounds(glyf)
        advance, _ = hmtx[name]
        hmtx[name] = (advance, glyph.xMin)
        top, bottom = max(top, glyph.yMax), min(bottom, glyph.yMin)
    font["hhea"].ascent, font["hhea"].descent = top, bottom
    font["OS/2"].sTypoAscender, font["OS/2"].sTypoDescender = top, bottom
    font["OS/2"].usWinAscent, font["OS/2"].usWinDescent = top, -bottom
    return font


# Where icon_table.rs puts a code point: its distance from CODE_BASE, in
# CODES' 16 bits.
CODE_BASE = 0xEA00
# What a plugin file, which asks the firmware, does not hold.
LOOKED_UP = '#[cfg(not(all(paperpaw_plugin, target_os = "none")))]'


def name_hash(name):
    """A name's hash as `paperpaw_sdk::icons` counts it (FNV-1a, 32 bits)."""
    value = 0x811C9DC5
    for byte in name.encode("ascii"):
        value = ((value ^ byte) * 0x01000193) & 0xFFFFFFFF
    return value


def icon_table(names):
    """The text of icon_table.rs for `names` (name: code point).

    Without pointers (a table of string slices went partly into RAM on
    the X4) and without the names (#258: 64 KB the firmware never showed):
    each name's hash, ascending, and its code point, 6 bytes a name. The
    names themselves, for tools and tests (`icons::names`): one text
    of all, sorted, a name that begins the next one kept only in it (`a-b`
    in `a-b-2`), and where each starts and how long it is.
    """
    ordered = sorted(names.items())
    if not all(name.isascii() and name for name, _ in ordered):
        sys.exit("an icon name is empty or not ASCII")
    hashed = {}
    for name, code in ordered:
        value, distance = name_hash(name), code - CODE_BASE
        if value in hashed:
            sys.exit(f"icons {hashed[value][0]} and {name} have one hash: icon_table.rs needs another")
        if not 0 <= distance <= 0xFFFF:
            sys.exit(f"icon {name} (U+{code:04X}) does not fit icon_table.rs' 16 bits")
        hashed[value] = (name, distance)
    text, starts = "", []
    for i, (name, _) in enumerate(ordered):
        following = ordered[i + 1][0] if i + 1 < len(ordered) else ""
        # A name that begins the next starts where that one, written
        # next, does.
        starts.append(len(text))
        if not following.startswith(name):
            text += name
    if len(text) > 0xFFFF:
        sys.exit(f"the names take {len(text)} bytes: more than STARTS' u16 reach")
    if max(len(name) for name, _ in ordered) > 0xFF:
        sys.exit("an icon name is longer than LENGTHS' u8 says")
    lines = [
        "// Generated by tools/icons/subset.py from Tabler Icons "
        f"{TABLER}; do not edit.",
        "",
        "/// Where an icon's code point is counted from (see [`CODES`]).",
        LOOKED_UP,
        f"pub(crate) const CODE_BASE: u32 = 0x{CODE_BASE:X};",
        "",
    ]
    lines += array(
        "The hashes ([`hash`]) of the names of all of Tabler's icons, ascending,\n"
        "/// no two the same. The icon font holds the icons PaperPaw uses, packs add\n"
        "/// others.",
        "HASHES",
        "u32",
        sorted(hashed),
        LOOKED_UP,
    )
    lines += array(
        "The code point of the icon whose name's hash is `HASHES[i]`: its\n"
        "/// distance from [`CODE_BASE`].",
        "CODES",
        "u16",
        [hashed[value][1] for value in sorted(hashed)],
        LOOKED_UP,
    )
    lines += [
        "/// The names of all of Tabler's icons, sorted, one after another; a",
        "/// name that begins the next one is kept only in it.",
        "pub(crate) static NAMES: &str = concat!(",
    ]
    lines += [f'    "{text[i:i + 80]}",' for i in range(0, len(text), 80)]
    lines += [");", ""]
    lines += array("Where the `i`th name starts in [`NAMES`].", "STARTS", "u16", starts)
    lines += array(
        "How long the `i`th name is.",
        "LENGTHS",
        "u8",
        [len(name) for name, _ in ordered],
    )
    return "\n".join(lines[:-1]) + "\n"


def array(doc, name, kind, values, attribute=None):
    """A static array, its values as many to a line as rustfmt puts."""
    out = [f"/// {doc}"] + ([attribute] if attribute else [])
    out += [f"pub(crate) static {name}: [{kind}; {len(values)}] = ["]
    line = "   "
    for value in values:
        item = f" {value},"
        if len(line) + len(item) > 100:
            out.append(line)
            line = "   "
        line += item
    out += [line, "];", ""]
    return out


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    root = Path(__file__).resolve().parents[2]
    names = tabler_codes(sys.argv[1])
    missing = [name for name in ICONS if name not in names]
    if missing:
        sys.exit(f"not in Tabler Icons {TABLER}: {', '.join(missing)}")
    codes = {name: names[name] for name in ICONS}

    font = icon_font(sys.argv[1], codes.values())
    # Named for what it is now.
    for record in font["name"].names:
        if record.nameID in (1, 4, 6):
            record.string = "PaperPaw Icons" if record.nameID != 6 else "PaperPawIcons"
    out = root / "plugins/fonts-default/fonts/PaperPawIcons.ttf"
    font.save(out)

    # Every icon's name, so that plugins name any of them; the drawings of
    # those the font lacks come from icon packs (tools/icons/pack.py).
    table = root / "crates/paperpaw-sdk/src/icon_table.rs"
    table.write_text(icon_table(names), encoding="utf-8", newline="\n")
    print(f"{len(codes)} icons in the font, {len(names)} names, {out.stat().st_size} bytes")


if __name__ == "__main__":
    main()
