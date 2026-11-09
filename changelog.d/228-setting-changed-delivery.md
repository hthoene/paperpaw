### Added

- **`event.setting_changed` reaches native plugins** (#228). A plugin that registers for event 64 is told of every kept change of a setting (a set, a reset, a preset, a write into a book's scope), with the old value as far as it could read the key with `settings.read`; a secret of another plugin and a grant are not announced. A scope switch, an import or an overflow arrives as one event with an empty key ("read again what you keep"). Each subscriber has its own bounded queue, told between callbacks, a few events to a plugin in a pass.
