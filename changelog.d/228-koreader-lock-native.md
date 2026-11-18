### Changed

- **KOReader exchange and screen lock start their move to interface 7** (#228). KOReader's file reading and writing, and the screen lock's rules (the code kept as a salted hash, the wait after five wrong tries, the keypad and the pattern), written so that a refused allocation never aborts and a secret is wiped when done, exist natively beside the old code, with the lock's settings page; the exchange, the lock screen and the touch lock follow when the host pieces they need arrive.
