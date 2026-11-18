### Fixed

- **A native plugin's "shown if" works on the settings page** (#257). `core.registry` ignored a native plugin's declared `shown_if`, so a dependent setting always showed (`reg_setting` bit 9), and the governing setting lacked bit 10, so the page did not re-read when it changed. Sleep and picture-home declare such settings. Both bits now come from the declaration store, as in Beta 3.
