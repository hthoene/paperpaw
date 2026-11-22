### Fixed

- **A tap on a list that just changed no longer stops the plugin** (#484).
  A tap where a row was drawn before the list shrank (Wi-Fi rereading its
  networks, for example) could end the plugin; now it does nothing. The −
  and + of a faint number row no longer change it either.
- **A faint row or small print with the focus is readable** (#484). Where
  the focus rested on one (a page with nothing to choose), it was drawn on
  the black focus bar in grey or invisible ink; now it is drawn as it is.
- **The page keys turn to a page of text alone again** (#484). A page of a
  list with nothing to choose on it (only text, such as the start of a
  message) was skipped by the next and previous page keys; it is shown
  again, as in Beta 3.
- **A section card goes on closed under its repeated heading** (#484). With
  section cards, a section that went on over a page break was drawn open at
  the top of the next page although its heading is shown again above it;
  it is closed there now, as in Beta 3.
