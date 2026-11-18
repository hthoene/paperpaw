### Added

- Added the Thai UI language pack `language-th` (ไทย; a machine translation, marked as a draft) and the letters pack `script-thai` it needs (Noto Serif Thai and Noto Sans Thai, SIL OFL 1.1) for the store. Vowels and tone marks above and below a letter, also a tone mark over an upper vowel, are placed by the font's anchors (`paperpaw-text` reads Thai's `mark` and `mkmk` lookups as it does Arabic's and Hebrew's). Thai has no spaces, so the pack's texts hold a zero width space after each word, where lines break (`tools/thai-segment.py`): no word list in the firmware (#190).
