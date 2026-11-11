### Fixed
- Settings and sleep (interface 7): a value is read into a buffer of 4,096 bytes, the most `settings.read` takes; the 4,160 bytes asked before were refused as invalid, so the settings screen could not open.
