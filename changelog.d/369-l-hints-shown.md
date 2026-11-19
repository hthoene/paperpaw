### Added

- The environment snapshot grows by an extension: `hints_shown` (SDK `Snapshot::hints_shown`) says whether the host's row of hints was on screen when the screen was last drawn, so a screen whose first press only shows its bars no longer keeps a timer of its own. Picture Home's bar uses it. Older plugins read the longer snapshot unchanged (#369 L, #205).
