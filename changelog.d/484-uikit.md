### Fixed

- **A tap on a list that just changed no longer stops the plugin** (#484).
  A tap where a row was drawn before the list shrank (Wi-Fi rereading its
  networks, for example) could end the plugin; now it does nothing. The −
  and + of a faint number row no longer change it either.
- **A faint row or small print with the focus is readable** (#484). Where
  the focus rested on one (a page with nothing to choose), it was drawn on
  the black focus bar in grey or invisible ink; now it is drawn as it is.
