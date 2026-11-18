### Added

- **Plugins can declare setup steps** (#228). A native plugin's `setup.step` records are admitted instead of refused as unsupported. A step's name must be its own: a plugin whose step is named like one of its screens, actions, settings or menu sections is refused, so one state push never sets two things. Steps go away with their plugin. The setup list and a step's help text (registry operations 15 and 22) follow when their contract is merged.
