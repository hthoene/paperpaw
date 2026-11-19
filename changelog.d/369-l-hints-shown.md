### Added

- The environment snapshot grows by an extension: `hints_shown` (SDK `Snapshot::hints_shown`) says whether the host's row of hints was on screen when the screen was last drawn, so a screen whose first press only shows its bars no longer keeps a timer of its own. Picture Home's bar uses it. Older plugins read the longer snapshot unchanged, and one that asks for exactly the 52-byte first layout gets that prefix without the extension: extensions are appended and only written when the buffer has room for them; only a buffer below 52 bytes is refused (#369 L, #205).
