"""Draws PaperPaw's Android launcher icon: the paper-coloured paw on a green sheet with a
dog-ear, as shells/android/icon.svg has it, at every launcher density.

Usage: python tools/android-icon.py   (needs Pillow; writes
shells/android/res/mipmap-*/ic_launcher.png and the 512 px icon of the
app stores' metadata, fastlane/metadata/android/en-US/images/icon.png)
"""

from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
RES = ROOT / "shells/android/res"
SIZES = {"mdpi": 48, "hdpi": 72, "xhdpi": 96, "xxhdpi": 144, "xxxhdpi": 192}
# Drawn this much larger, then reduced: smooth edges at every size.
SCALE = 8
# The logo's colours: the sheet, its dog-ear, the paw.
SHEET = (0x3B, 0x62, 0x5F, 255)
DOG_EAR = (0x24, 0x40, 0x3C, 255)
INK = (0xE6, 0xE0, 0xD4, 255)


def quad(p0, p1, p2, steps=24):
    """Points along the quadratic curve from p0 over p1 to p2."""
    return [
        ((1 - t) ** 2 * p0[0] + 2 * (1 - t) * t * p1[0] + t * t * p2[0],
         (1 - t) ** 2 * p0[1] + 2 * (1 - t) * t * p1[1] + t * t * p2[1])
        for t in (i / steps for i in range(steps + 1))
    ]


def icon(size: int) -> Image.Image:
    big = size * SCALE
    image = Image.new("RGBA", (big, big), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    # The drawing's units, 12 to 108 as in icon.svg, fill 88 % of the
    # icon, as launchers expect.
    inset = big * 0.06
    unit = (big - 2 * inset) / 96

    def at(x, y):
        return (inset + (x - 12) * unit, inset + (y - 12) * unit)

    def shape(points):
        return [at(x, y) for x, y in points]

    sheet = ([(24, 12), (82, 12), (108, 38)] + quad((108, 96), (108, 108), (96, 108))
             + quad((24, 108), (12, 108), (12, 96)) + quad((12, 24), (12, 12), (24, 12)))
    draw.polygon(shape(sheet), fill=SHEET)
    fold = [(82, 12)] + quad((82, 30), (82, 38), (90, 38)) + [(108, 38)]
    draw.polygon(shape(fold), fill=DOG_EAR)
    for cx, cy in [(40, 54), (58, 45), (76, 54)]:
        (x0, y0), (x1, y1) = at(cx - 7.5, cy - 7.5), at(cx + 7.5, cy + 7.5)
        draw.ellipse((x0, y0, x1, y1), fill=INK)
    pad = (quad((40, 79), (40, 65), (58, 65)) + quad((58, 65), (76, 65), (76, 79))
           + quad((76, 79), (76, 92), (58, 92)) + quad((58, 92), (40, 92), (40, 79)))
    draw.polygon(shape(pad), fill=INK)
    return image.resize((size, size), Image.LANCZOS)


def main() -> None:
    for density, size in SIZES.items():
        out = RES / f"mipmap-{density}" / "ic_launcher.png"
        out.parent.mkdir(parents=True, exist_ok=True)
        icon(size).save(out, optimize=True)
        print(out.relative_to(ROOT))
    store = ROOT / "fastlane/metadata/android/en-US/images/icon.png"
    store.parent.mkdir(parents=True, exist_ok=True)
    icon(512).save(store, optimize=True)
    print(store.relative_to(ROOT))


if __name__ == "__main__":
    main()
