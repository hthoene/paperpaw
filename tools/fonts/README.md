# Fonts for data plugins

Source fonts of the packs `tools/script-pack.py` builds for the card;
the firmware holds none of them.

| File | Font | Licence | Source |
|---|---|---|---|
| `NotoNaskhArabic.ttf` | Noto Naskh Arabic, variable (`wght` 400 to 700) | SIL OFL 1.1, no Reserved Font Name (`NotoNaskhArabic-OFL.txt`) | `ofl/notonaskharabic/NotoNaskhArabic[wght].ttf` of github.com/google/fonts at 6c1dbe27 |
| `NotoSerifHebrew.ttf` | Noto Serif Hebrew, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifHebrew-OFL.txt`) | `ofl/notoserifhebrew/NotoSerifHebrew[wdth,wght].ttf` of github.com/google/fonts at 6c1dbe27 |
| `NotoSerifBengali.ttf` | Noto Serif Bengali, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifBengali-OFL.txt`) | `ofl/notoserifbengali/NotoSerifBengali[wdth,wght].ttf` of github.com/google/fonts at 7af0d90b |
| `NotoSerifDevanagari.ttf` | Noto Serif Devanagari, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifDevanagari-OFL.txt`) | `ofl/notoserifdevanagari/NotoSerifDevanagari[wdth,wght].ttf` of github.com/google/fonts at 7af0d90b |
| `NotoSerifEthiopic.ttf` | Noto Serif Ethiopic, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifEthiopic-OFL.txt`) | `ofl/notoserifethiopic/NotoSerifEthiopic[wdth,wght].ttf` of github.com/google/fonts at 6c1dbe27 (SHA-256 `49d5daa6…`) |
| `NotoSerifThai.ttf`, `NotoSansThai.ttf` | Noto Serif Thai and Noto Sans Thai, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifThai-OFL.txt`, `NotoSansThai-OFL.txt`) | `ofl/notoserifthai/NotoSerifThai[wdth,wght].ttf` and `ofl/notosansthai/NotoSansThai[wdth,wght].ttf` of github.com/google/fonts at 6c1dbe27 (SHA-256 `34a7ad11…` and `5a1c559b…`) |
| `NotoSerifKhmer.ttf`, `NotoSerifLao.ttf` | Noto Serif Khmer and Noto Serif Lao, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifKhmer-OFL.txt`, `NotoSerifLao-OFL.txt`) | `ofl/notoserifkhmer/NotoSerifKhmer[wdth,wght].ttf` and `ofl/notoseriflao/NotoSerifLao[wdth,wght].ttf` of github.com/google/fonts at 6c1dbe27 (SHA-256 `50ad0320…` and `c35bb3fb…`) |
| `NotoSerifTibetan.ttf` | Noto Serif Tibetan, variable (`wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifTibetan-OFL.txt`) | `ofl/notoseriftibetan/NotoSerifTibetan[wght].ttf` of github.com/google/fonts at 6c1dbe27 (SHA-256 `060ec022…`) |
| `NotoSerif-subset.ttf`, `NotoSans-subset.ttf` | Noto Serif and Noto Sans, variable (`wdth`, `wght`), cut to the 12 letters Literata or Source Sans 3 lack for `script-latin-extended` (Ɓ ɓ Ɗ ɗ Ƙ ƙ Ƴ ƴ Ǹ ǹ Ḿ ḿ; layout tables dropped) | SIL OFL 1.1, no Reserved Font Name (`NotoSerif-OFL.txt`, `NotoSans-OFL.txt`) | `ofl/notoserif/NotoSerif[wdth,wght].ttf` and `ofl/notosans/NotoSans[wdth,wght].ttf` of github.com/google/fonts at cc5661d4, made with fontTools (`subset`) |
| `NotoSerifGeorgian.ttf`, `NotoSerifArmenian.ttf` | Noto Serif Georgian and Noto Serif Armenian, variable (`wdth`, `wght`) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifGeorgian-OFL.txt`, `NotoSerifArmenian-OFL.txt`) | `ofl/notoserifgeorgian/NotoSerifGeorgian[wdth,wght].ttf` and `ofl/notoserifarmenian/NotoSerifArmenian[wdth,wght].ttf` of github.com/google/fonts at cc5661d4 (SHA-256 `a13a90c4…` and `9354b743…`) |
| `ICU-word-lists-LICENSE.txt` | the licences of ICU's Khmer and Lao word lists (`tools/word-table.py`) | Unicode License V3; the Lao list's BSD-style licence | `LICENSE` of github.com/unicode-org/icu (its Unicode License V3 and the Lao dictionary's part) |
| `NotoSerifSC-subset.ttf`, `NotoSerifTC-subset.ttf`, `NotoSerifJP-subset.ttf` | Noto Serif SC, TC and JP, static at weight 400, cut to the characters of the packs (6,921, 5,559 and 3,345 code points: GB 2312, Big5 level 1, the Joyo kanji and JIS X 0208 level 1 with kana, CJK punctuation, full-width forms; layout tables dropped) | SIL OFL 1.1, no Reserved Font Name (`NotoSerifCJK-OFL.txt`, the text google/fonts ships) | `ofl/notoserifsc/NotoSerifSC[wght].ttf`, `ofl/notoseriftc/NotoSerifTC[wght].ttf`, `ofl/notoserifjp/NotoSerifJP[wght].ttf` of github.com/google/fonts at 6c1dbe27, made with fontTools (`varLib.instancer` wght=400, `subset` with the sets of `tools/fontgen/sets`: `hanzi-gb2312.txt`, `hanzi-big5.txt`, `kanji-japanese.txt`) |
| `NanumMyeongjo-Regular.ttf` | Nanum Myeongjo Regular, static | SIL OFL 1.1, **with** Reserved Font Names (`NanumMyeongjo-OFL.txt`): packs made from it are not named after it (NOTICE) | `ofl/nanummyeongjo/NanumMyeongjo-Regular.ttf` of github.com/google/fonts at 6c1dbe27 (SHA-256 `7ed9e865…`) |

Greek (`script-greek`) and Cyrillic (`script-cyrillic`) have no file
here: they are drawn in Literata's own faces
(`plugins/fonts-default/fonts`, SIL OFL 1.1, `Literata-OFL.txt`), which
hold modern and polytonic Greek and the Cyrillic block (U+0400 to U+04FF).

Nanum Myeongjo is the book face for Korean (#61; `paperpaw-notes`
design `korean-support.md` §4): a classic Myeongjo, light enough for
the small sizes. `script-korean` holds the 2,350 Hangul syllables of
KS X 1001 (`tools/fontgen/sets/hangul-ks.txt`), which ordinary modern
text keeps to, with CJK punctuation, compatibility jamo and full-width
forms: 8.2 MiB for one face in seven sizes (all 11,172 syllables would
be 38 MiB). It extends Literata's regular face only; text in bold,
italic or the interface's font finds Hangul there, as the reading font
is where a font lacking a letter looks last.

Noto Naskh Arabic is the reading font for Arabic, Persian and Urdu
(#33): it holds the presentation forms with their dots, which packs draw
joined letters with. Noto Serif Hebrew goes with Literata's serifs.

Thai (`script-thai`, #190): Noto Serif Thai for Literata's four faces
(weights 400 and 700, upright for all four), Noto Sans Thai for the
interface's font; the 87 letters, vowels, tone marks and digits of
U+0E01 to U+0E5B (no Thai keyboard yet). The vowels and tone marks above
and below a letter are placed by the fonts' GPOS `mark` and `mkmk`
anchors, like Hebrew's points (a tone mark over an upper vowel stands on
it); the forms a font gives a tone mark by contextual substitution (over
a tall letter, left of sara am) are not applied. Thai has no spaces:
`tools/thai-segment.py` puts a zero width space after each word of the
language pack's texts, where the line breaker breaks.

Khmer, Lao and Tibetan (`script-khmer`, `script-lao`, `script-tibetan`,
#314): their blocks from Noto Serif Khmer, Lao and Tibetan, shaped as
Bengali's (`crates/paperpaw-text/src/indic`): Khmer's coeng subscripts,
ro and the vowel signs drawn left of their consonant, split vowels; Lao's
vowels and tone marks above and below by the font's forms and anchors;
Tibetan's stacks. Noto Serif Tibetan's substitutions are too many for a
pack read from a card to hold: its packs keep the 600 clusters most
frequent in the Tibetan Wikipedia shaped (`tools/tibetan-clusters.py`),
a rarer one is drawn as the longest of those it starts with. Khmer and
Lao have no spaces between words: their packs carry the 3,000 words of
ICU's lists most frequent in their Wikipedias (`tools/word-table.py`),
where lines break; Tibetan breaks after the tsheg ending each syllable.

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

Chinese and Japanese (#3645) are drawn in Noto Serif: `script-chinese-simplified` (the 6,763 ideographs of GB 2312), `script-chinese-traditional` (the 5,401 frequently used characters of Big5) and `script-japanese` (hiragana, katakana, the 2,998 kanji of the Joyo list and JIS X 0208 level 1), each with CJK punctuation and the full-width forms, in four sizes (20, 24, 28, 32 px: what the firmware's fonts have; larger text is drawn at 32) and two faces as Korean's (Literata's regular and the interface's font, so 19.8, 16.0 and 9.4 MiB on the card). The lists come from the national standards through Python's codecs (`gb2312`, `big5`, `euc_jp`) and Unicode's Unihan (`kJoyoKanji`), so they hold no copied tables. A character a book uses outside a list shows as a box, as in Korean.

Bengali (`script-bengali`) and Devanagari (`script-devanagari`, #157) are
drawn in Noto Serif Bengali and Noto Serif Devanagari, shaped: the packs
hold the fonts' conjuncts, half forms, reph and vowel-sign variants as
glyphs without code point, and their GSUB features (`nukt`, `akhn`,
`rphf`, `blwf`, `half`, `pstf`, `cjct`, `pres`, `abvs`, `blws`, `psts`,
`haln` ...) as the pack's substitution table, which `paperpaw-text`'s
Indic shaper applies a word at a time; the marks' anchors come from
`abvm` and `blwm`. These scripts have no italics: a letters pack holds
one file for each weight, each extending all the faces it stands for
(Literata's regular and italic and the interface's font; the bold ones),
opened once. A table is kept within 32 KiB of 4-byte keys and offsets, what
a pack read from the card keeps in memory: Devanagari's leaves out the five
contextual lookups that choose among the lengths of the vowel sign i by
the consonants after it (ि is then drawn in its usual length). The table
is then written with 2-byte keys and offsets (version 2), which halves it
in memory: Bengali 8.4 KiB, Devanagari 15.9 KiB.
