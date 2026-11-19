### Fixed

- The UI language packs for Amharic, Belarusian, Central Kurdish, Kazakh, Kyrgyz, Macedonian, Pashto and Serbian build again: their longest help texts (calendar, vocabulary and a few others) were over the 4,096 bytes a compiled catalog holds per message and are condensed, all their content kept; `tools/language-pack.py check` now reports such messages (#385).
