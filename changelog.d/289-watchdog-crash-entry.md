### Fixed

- **A watchdog or brownout restart on the X4 now leaves an entry in `crashes.txt`** (#289). Only a panic did before, so a hang that the watchdog ended left no trace. The next start writes the chip's reset reason (RTC or timer-group watchdog, super watchdog, brownout) with the place the firmware last noted being at and the light sleeps of the run.
