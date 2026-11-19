### Added

- Burmese letters (`script-myanmar`, Noto Serif Myanmar with its words for breaking lines) and the interface in Burmese (`language-my`, a machine draft; it needs the letters) are in the store (#401, #397). Burmese words are shaped as HarfBuzz shapes them (2,000 of 2,000 of the most frequent).

### Changed

- Reading in scripts written without spaces takes less memory: a paragraph keeps room for the places it may break at, not one for every letter (24 KiB for a long Burmese, Khmer or Thai paragraph, now 4 KiB) (#401). A page's list of lines and each line's list of faces are as long as they are. On the X4 a Burmese book now peaks at 211 KiB (249 KiB with its whole table before), Khmer at 199 KiB (221), Hindi at 208 KiB (215).
- Script packs keep only the forms text of their letters can come to: Burmese's substitutions take 20.3 KiB (31.4), Devanagari's 14.8 KiB (16.6), Sinhala's 2.9 KiB (5.1); text is shaped as before (#401). Packs made before read as before.
