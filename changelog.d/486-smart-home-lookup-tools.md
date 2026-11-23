### Fixed

- **Smart home keeps the chosen entities when the card cannot be read**
  (#486). A failed read of the saved list was taken as an empty list, and the
  next fetch saved that empty list over it. Now the list is read again and
  never replaced until it was read; a save the card refuses is said, and a
  list of states cut short changes no entity.
- **Closing the Wikipedia summary no longer fails the dictionary panel's
  lookup** (#486). Both used one request token, so closing the summary also
  cancelled the panel's fetch and the panel said it had failed.
- **Translation says when the DeepL key could not be saved** (#486), instead
  of asking for it again next time without a word.
- **A read-later fetch cut short by the end of its background time is no
  longer reported as done** (#486); it says the rest follows with the next
  fetch.
