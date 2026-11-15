### Fixed

- **The groups in Settings show their icons again** (#268). Since the native Settings, every group row was drawn without an icon: the registry's reply for setting groups carried none. `service.reg_group` gains the optional `icon` (the name in the icon font, at most 64 bytes; appended, so a reader that does not know it skips it), `RegistryGroup::icon()` reads it, and Settings shows it. A group nobody declared an icon for gets `puzzle`, as in Beta 3.
