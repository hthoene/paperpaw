### Fixed

- **The native plugin store restarts PaperPaw again** (#244). *Restart now* and *Switch on and restart* were refused by PaperPaw because the store did not name the permission they need; it does now.
- **The first-run offer opens as it did in Beta 3** (#244). Opened with nothing to add, it shows its *Done* row and stays, instead of closing at once.
- **Installing from the native plugin store works** (#244). The download could not start (it said "could not be downloaded") because the store could not make its staging folder; and after a declined install the plugin's page stayed on *Getting ready…*. Both are fixed.

### Added

- **Discover fonts is back on the Text page of the reading settings** (#244). It opens the fonts browser's list, or says why it cannot (not installed, switched off, waiting for a restart, could not start) and offers the one thing to do about it, in English and German.

### Changed

- **The Plugin store no longer fetches Literata's bold and italic at the first start** (#258, Decision 186). The lean firmware has no Literata of its own any more; `literata-styles` is in the store like any other plugin.
