### Fixed

- **Library: progress of the first scan and a hanging catalog** (#460). While the library is read for the first time it shows the books found so far again (as in Beta 3), not "Library is being prepared" until the end. When the catalog work stops making progress for 90 seconds, or a catalog that was asked for never starts, the library now says the card cannot be read instead of waiting without end. The time is checked only when the library asks; no timer wakes the device, and time asleep or under an open book does not count.
