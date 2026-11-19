### Changed

- **Page turns measure and hyphenate far less** (#399). The reader forgot the kerning of about a third of the letter pairs it had learned and a good half of the words it had asked the hyphenation table about from one page to the next, and asked the font and the card again at every turn. The table of kerned pairs now holds 256 entries in sets of two (1,024 bytes, as before) and the break places of 256 words in sets of two (2,056 bytes, 8 fewer than before). On the emulator a turn of a Latin book now averages about 45 text measures (65 before) and 7 hyphenation lookups (12 before).
