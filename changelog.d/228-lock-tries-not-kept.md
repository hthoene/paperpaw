### Fixed

- **The lock says when it cannot keep the wrong tries** (#257). A card that refuses the count of wrong codes (full, read only) used to be ignored without a word, so the count was lost at the next start. It is now said (in English and German) at the first wrong try the card refuses, and not again until a write went through. The attempt itself is still not refused: a card that cannot be written must not lock the owner out.
