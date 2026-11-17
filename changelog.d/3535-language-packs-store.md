### Added

- Added the UI language packs Czech, Danish, Finnish, French, Hungarian, Indonesian, Italian, Norwegian Bokmål, Dutch, Polish, Brazilian Portuguese, Slovak, Swedish and Turkish as data plugins (`language-<code>`) for the store, and completed Spanish and Arabic against the current English texts, including the kernel's settings and install dialogs. The packs are machine translations, marked as drafts in their descriptions (#3535).

### Fixed

- `tools/language-pack.py` now also reads the kernel's `i18n/host` and `i18n/settings` folders and the app's `i18n/pebble`, which a language pack lacked until now.
