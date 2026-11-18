### Changed

- Hyphenation keeps the answers of the last 64 words asked of each open language table (1 KiB a table, dropped with it): a word asked again, such as a name fitted in every band of a frame or a list drawn again, no longer reads the table from the card. 20 words asked 50 times read the card 363 times instead of 8,163.

### Fixed

- `paperpaw-dev perf` over all plugins measures every screen: the firmware confirmation, which guards the device, stayed open after it was measured, and every later screen opened below it unseen until the screen stack was full (about 70 screens were left out without an error). A screen that still cannot be measured is now an error that names it and the screen shown instead, and the run ends with a failure.
