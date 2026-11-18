### Changed

- **The fonts browser reads its catalog without aborting when memory is short** (#228). The JSON
  reader's own allocations now refuse cleanly too, so a short heap ends the read with an error
  instead of stopping the plugin.
