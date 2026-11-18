### Changed

- Native plugins whose declarations are all read from their read-only
  image (linked plugins and mapped card modules) keep no per-plugin index of
  their records any more: their records of a kind are found by walking the
  image's batches, which saves about 6.6 KiB of heap on an X4 with the
  linked plugins (#248). Records are visited in the order the plugin
  declared them, for every plugin; lists built from them (e.g. the widgets a
  place can show) follow that order instead of a name-hash order.
