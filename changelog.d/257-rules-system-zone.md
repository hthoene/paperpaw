### Fixed

- **Rules for a time of day follow the device's time zone again** (#257). With the time zone on "automatic" (the default), the native rules plugin counted the time in UTC, so a rule for 7:00 ran at the wrong hour outside UTC; it now uses the system's zone, as Beta 3 did.
