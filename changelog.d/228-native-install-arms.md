### Added

- **A native plugin can declare settings, setting groups and actions** (#228). The kernel installs them with the plugin's screens in one step: if any record is refused, nothing is published. When the plugin unloads, its actions, settings and groups go too (the values the reader chose stay for its next load).
