### Fixed

- **`paperpaw-dev` shows the native home screen and settings.** The tool built the device model without the native plugins, so its kernel had no home screen and the checks that place a widget on it (the gallery, the screen and widget sessions, the performance probe) failed. It now links them, as the app does by default.
