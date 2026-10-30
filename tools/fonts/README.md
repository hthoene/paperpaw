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
