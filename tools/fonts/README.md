# Fonts for data plugins

Source fonts of the packs `tools/script-pack.py` builds for the card;
the firmware holds none of them.

| File | Font | Licence | Source |
|---|---|---|---|
| `NotoNaskhArabic.ttf` | Noto Naskh Arabic, variable (`wght` 400 to 700) | SIL OFL 1.1, no Reserved Font Name (`NotoNaskhArabic-OFL.txt`) | `ofl/notonaskharabic/NotoNaskhArabic[wght].ttf` of github.com/google/fonts at 6c1dbe27 |
| `NotoSerifHebrew.ttf` | Noto Serif Hebrew, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifHebrew-OFL.txt`) | `ofl/notoserifhebrew/NotoSerifHebrew[wdth,wght].ttf` of github.com/google/fonts at 6c1dbe27 |

Noto Naskh Arabic is the reading font for Arabic, Persian and Urdu
(#33): it holds the presentation forms with their dots, which packs draw
joined letters with. Noto Serif Hebrew goes with Literata's serifs.

The packs keep where marks sit on letters (Arabic harakat, Hebrew
points): `paperpaw-fontgen` reads the fonts' GPOS `mark` and `mkmk`
anchors into each pack's anchors table, for the form each letter is
drawn in and under or over its dots. A mark whose classes its letter
does not hold (a letter from another font) is centred over it; marks
the fonts give no anchors keep the place their glyph has.

A pack has the sizes it was made in: Literata's seven (20, 24, 28, 32,
36, 40 and 48 px). The X4's firmware holds Literata up to 32 px; the
store's `literata-large` adds 36 to 48. A pack of another script adds
letters, not sizes: the reader's largest size stays Literata's (32 px,
48 with `literata-large`), so the packs' sizes from 36 px serve readers
with that plugin. Text asked for at a size between two is drawn at the
nearer one (at the smaller where both are as near: 22 px at 20), Latin
and Arabic or Hebrew alike, its marks by that size's anchors.
