### Fixed

- **The lock says when it cannot keep the wrong tries** (#257). A card that refuses the count of wrong codes (full, read only) used to be ignored without a word, so the count was lost at the next start. It is now said once, in English and German. The attempt itself is still not refused: a card that cannot be written must not lock the owner out.
