### Fixed

- **Highlights stay on their own page after a turn back** (#265). After a turn
  back (or a jump) the reader marked the page, and chose its words, by the
  words of the page it had left: a highlight was missing on its own page, could
  stand on another page over other text, and a word chosen there was not the
  word shown. The page's words are now made again for every page shown.
- **Short highlights of an earlier version are drawn again** (#265). A highlight
  of a word or two ("Om", "the") that a Beta 3 card or another conversion of the
  book wrote was never drawn. It is drawn where it covers exactly its words.
- **The selection's actions do what they say** (#265). `reader.selection.highlight`,
  `.note`, `.look-up`, `.copy`, `.extend` and `.cancel` (a key bound to one, a
  remote) were answered and did nothing; they now do what the bar's button does.
- **A highlight is dots again on black and white screens** (#265). Its light
  gray was drawn as rows that ran through the letters; it is a pattern of dots as
  in Beta 3, as dark as before. A host without the pattern fill draws rows.
