### Fixed

- **Update trial and safe mode run the built-in plugins** (#457). On the X4s an update's 30-second trial now starts Home, the reader, Wi-Fi, the update and the other plugins built into the firmware, so a crash in one of them during the trial rolls the update back (also for the update from Beta 3). Safe mode runs them too, except the one the crash was in: Wi-Fi and the update stay to fetch a fixed firmware without a computer. Code plugins from the card still wait for a normal start.
