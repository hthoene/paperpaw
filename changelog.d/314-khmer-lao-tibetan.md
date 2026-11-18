### Added

- Khmer, Lao and Tibetan books (#314): the store's `script-khmer`, `script-lao` and `script-tibetan` bring their letters from Noto Serif Khmer, Lao and Tibetan, shaped as Bengali and Devanagari are. Khmer's consonants subscript under coeng, ro and the vowel signs drawn left of their consonant move before it, its split vowels are drawn in two parts; Lao's vowels and tone marks stack above and below their letter; Tibetan's consonants stack. Khmer and Lao are written without spaces: their packs carry the 3,000 words of ICU's word lists (Unicode License V3) most frequent in their Wikipedias (`tools/word-table.py`), and lines break between them as Thai's do; Tibetan breaks after each syllable's tsheg. Noto Serif Tibetan's stacks are too many for a pack read from a card to hold: its packs keep the 600 clusters most frequent in the Tibetan Wikipedia shaped (`tools/tibetan-clusters.py`; 99.8 % of its text), a rarer one drawn as the longest of those it starts with. The reader offers the packs for books in these scripts.

### Changed

- The substitutions of a script pack skip marks where the font's lookups say so (OpenType's lookup flags: ignoring marks, mark filtering sets, mark attachment classes), as sets listed at the table's end; packs made before read as they did.
