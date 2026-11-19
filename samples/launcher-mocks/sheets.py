#!/usr/bin/env python3
"""Puts the screenshots of each concept side by side: out/sheet-<concept>.png."""
import sys
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

out = Path(__file__).parent / "out"
SHEETS = {
    "a": ("A  Words", ["a-home", "a-library", "a-quick", "a-home-de", "a-home-largest"]),
    "b": ("B  Tiles", ["b-home", "b-library", "b-quick", "b-home-de", "b-home-largest"]),
    "c": ("C  One book", ["c-home", "c-shelf", "c-more", "c-home-de", "c-home-largest"]),
    "d": ("D  Panel", ["d-home", "d-panel", "d-library", "d-home-de", "d-home-largest"]),
}
NOTES = {
    "a": ["Home", "Library", "Quick settings"],
    "b": ["Home", "Library", "Quick settings"],
    "c": ["Home", "Shelf", "More (Menu button)"],
    "d": ["Home", "Panel", "Library"],
}
LAST = ["German, focus moved on", "Largest text size (24 pt)"]
try:
    font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 22)
except OSError:
    font = ImageFont.load_default()
for key, (title, names) in SHEETS.items():
    shots = [Image.open(out / f"{name}.png").convert("RGB") for name in names]
    w, h = shots[0].size
    gap, top = 24, 56
    sheet = Image.new("RGB", (len(shots) * (w + gap) + gap, h + top + gap), "white")
    draw = ImageDraw.Draw(sheet)
    for index, shot in enumerate(shots):
        x = gap + index * (w + gap)
        sheet.paste(shot, (x, top))
        draw.rectangle([x - 1, top - 1, x + w, top + h], outline="black")
        draw.text((x, 16), f"{title}: {(NOTES[key] + LAST)[index]}", fill="black", font=font)
    sheet.save(out / f"sheet-{key}.png")
    print(out / f"sheet-{key}.png", sheet.size)
