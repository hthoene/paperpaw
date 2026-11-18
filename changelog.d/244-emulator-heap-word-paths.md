### Fixed

- **`paperpaw-dev package` takes a plugin library with a heap word, as the emulator does** (#244): the check and the emulator's loader read one list of what a library may export (`PAPERPAW_ENTRY`, `PAPERPAW_METADATA`, `PAPERPAW_HEAP`). A heap word that overlaps the entry record or the metadata is refused.
