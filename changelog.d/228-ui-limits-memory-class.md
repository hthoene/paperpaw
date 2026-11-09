### Changed

- The `core.ui` limits and admission rules for the small heap (the X4 profile)
  now follow the memory class a device declares (`MemoryClass::Small`) instead
  of a list of device ids. `HostWiring::ui` takes the `MemoryClass`. A device
  that declares `Large` (the X4 Pro and X4 Classic) keeps the large limits.
