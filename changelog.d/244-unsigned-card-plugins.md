### Changed

- **Code plugins copied to the card run without being installed (Decision 205).** A plugin folder an author copies to the card runs its code at the next start, unsigned and without a question; the plugin manager can tell it apart (`reg_plugin.flags` bit 6, "not signed"). It does not run for an id that has an author's key pinned (a plugin once installed signed), nor when it declares installing plugins or firmware (`uses.install`, `uses.firmware`): those still need an install. An installed plugin still runs only the file installed.
