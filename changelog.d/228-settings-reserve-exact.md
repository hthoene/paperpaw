### Fixed

- **Changing a setting kept in the program no longer doubles the settings list** (#248). The first change to such a setting copies it to the heap; the list of copies is kept exactly sized, and that copy now takes room for one more instead of doubling the list (on the X4 profile with the statistics plugin's settings copied, a 35 KiB peak at start). Where the heap has no room for it, the change is refused like one with no place left.
