### Fixed

- **A hosted plugin keeps no second copy of its registrations** (#228). The shell reads the records through the committed batch instead of a copy kept beside it, which saves about 768 bytes per plugin on the X4 heap. The registrations and ticks a plugin gets are unchanged.
