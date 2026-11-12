### Added

- **"Not enough memory for this" when a screen cannot open for a lack of memory** (#248). The user saw nothing before (the log said it): a notice made then would meet the same lack. The kernel now makes this notice ahead, at start and again when the language or any interface setting (`core.ui-*`) changes, and shows it without allocating, in place of a notice that shows when a native screen (a parked plugin's among them) cannot open for a lack of memory. It comes back for the next time when it ends. English and German.
