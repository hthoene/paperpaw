### Fixed

- **Statistics counted UTC days** (#459). Reading between local midnight and the zone's offset (1–2 hours in Germany) went to the day before: the calendar, today's minutes, the streaks, *Books this year* and the reading goal's ring (which showed 0 minutes in those hours) were off. The days are local days again, as in Beta 3, in the zone chosen in the settings, else the device's (statistics 1.1.2).
