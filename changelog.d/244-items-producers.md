### Changed

- Kernel: its own contributions, plugins of data, the built-in fonts and the Pebble theme register through functions over `Items`; a plugin with the old `Plugin` trait is taken through an adapter. Nothing a user or a plugin sees changes (#244).
