### Changed

- **A linked plugin's declared services are no longer copied** (#248). The host borrows the list from the plugin's checked metadata, which is the program's read-only data, instead of copying keys and entries onto the heap for every linked plugin. Loaded card plugins still get a copy.
