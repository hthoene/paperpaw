### Added

- **Per-plugin heap budget and block owners** (#248). A plugin's block of 256 bytes or more carries
  a header (previous, next, owner index; 12 bytes on the X4) so that `dealloc` and `realloc` follow
  the block's own owner in constant time and the teardown sweep can list what a plugin still owns;
  a smaller block has no header and is charged by its size, rounded up to 8 bytes, to the plugin
  whose code runs (hundreds of small blocks would cost the X4 kilobytes of header). The owner pays
  for what it holds, alignment padding included. `HostWiring::plugin_heap(bytes)` sets the ceiling a
  shell gives each plugin (`PLUGIN_HEAP_SMALL` 48 KiB on the X4, `PLUGIN_HEAP_LARGE` 4 MiB;
  defaults follow the device's memory class). The limit is a ceiling, not a reservation, and may
  change after the device measurement.
