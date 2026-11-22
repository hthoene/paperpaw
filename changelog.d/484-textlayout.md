### Fixed

- **Khmer, Lao, Burmese and Indic lines with a zero width space between their words are drawn shaped again**, as wide as line breaking measured them, instead of letter by letter (#484).
- **Ruby and glosses that do not fit on a page's last line are cut there** instead of disappearing: a gloss does not go on over a page break, as on a paragraph's last line (#484).
- **A position inside a table row too wide for its columns opens on its own page again** (a bookmark, a search hit or the place kept after a font change), not on the row's first page (#484).
- **A gloss or ruby of a paragraph's first word is shown where the paragraph starts with a drop capital**, on the line beside the capital (#484).
