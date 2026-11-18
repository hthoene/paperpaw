### Changed

- **The built-in UI font and icons take about 254 KB less flash** (Decision 197). The two packs the firmware carries (PaperPaw Sans and PaperPaw Icons) now store their glyph bitmaps packed (runs of empty and full pixels as a run length, rows without padding), and the text drawing decodes a glyph a row at a time. The pictures are pixel for pixel the same; no screen changes. Packs on the card and the reading fonts keep their format and still load.
