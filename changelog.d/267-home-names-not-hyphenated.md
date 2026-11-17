### Changed

- **Home no longer hyphenates the names of its tools** (#267). On the X4 each question to the hyphenation table was a host call that read the table from the card (13 ms, 21 of them in the frame that returns to Home); a name that no line holds now ends in "…" instead of breaking inside a word. The tools drawer still hyphenates.
