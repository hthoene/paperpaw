### Fixed

- **A native plugin's `setting.extra` reaches the settings page** (todo 541, #257). `core.registry` read only a setting spec's own extra, and a native plugin's extra stays in the declaration store. Its int settings therefore lost the slider flag, and op 2 lost the unit, step, section, follows, shown-if and editor. These are now read from the store when a row is asked for, and nothing extra stays resident.
