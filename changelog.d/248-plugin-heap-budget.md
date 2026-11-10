### Added

- **Per-plugin heap budget and block header** (#248). Every block of the plugin heap now carries a
  header (previous, next, owner index; 12 bytes on the X4) so that `dealloc` and `realloc` follow
  the block's own owner in constant time, and the owner pays for what it holds, header and
  alignment padding included. `HostWiring::plugin_heap(bytes)` sets the ceiling a shell gives each
  plugin (`PLUGIN_HEAP_SMALL` 48 KiB on the X4, `PLUGIN_HEAP_LARGE` 4 MiB; defaults follow the
  device's memory class). The limit is a ceiling, not a reservation, and may change after the
  device measurement.
