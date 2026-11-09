### Fixed

- **A book's details are written without an infallible allocation** (#228). `paperpaw_sdk::library::save` (and what the library keeps itself, e.g. the `provide` of a source) makes the text of the details file with `try_reserve`, counts it against the 16 KiB limit before making it, and answers `OutOfMemory` or `TooLarge` instead of aborting; the text is the same as before.
