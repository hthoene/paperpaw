### Added

- **A native plugin's `slot.offer` reaches the slot registry** (#228). A screen a native plugin offers for a slot (`core.translate`, `core.lookup`, a slot of its own) is now bound like any other offer: `Env::has_slot` is true and `slot_screen`/`slot_screens` name it, so a reader that hides a feature until a plugin offers its slot shows it. The offers go with the plugin when it is unloaded, are admitted whole or not at all (no room refuses the plugin, never on a full heap), and where the heap has no room to update the list of bound slots the list stays and a warning says so.
