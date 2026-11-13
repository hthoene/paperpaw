### Added

- **Registry: plugins a store installed say so** (#244). `install_plugin` takes `flags` bit 2
  "from a catalog" (a store's install, code or data), which marks the version in the host's record
  (interface 6's `plugin-origin.txt`); a device that kept no record takes the plugins installed for
  the store's once at start, as interface 6's store did. `core.registry` ops 5 and 6 set
  `reg_plugin.flags` bit 5 where the version installed, or the one waiting, is in that record, and
  write that version as the store named it. The native store offers updates only for these.
