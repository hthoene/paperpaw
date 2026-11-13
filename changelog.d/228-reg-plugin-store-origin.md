### Added

- **Registry: plugins a store installed say so** (#244). `core.registry` ops 5 and 6 set
  `reg_plugin.flags` bit 5 where the version installed, or the one waiting, is one a store's
  install put in place (the host's own record), and write that version as the store named it.
  The native store offers updates only for these, as interface 6 did.
