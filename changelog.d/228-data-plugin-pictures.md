### Fixed

- **Pictures from a data plugin's folder** (#228). Settings and presets can name a file of a data plugin (the sleep picture `plugins/owls/owl.png`, a logo, a picture folder); native sleep could not show them because the card's `plugins/` is hidden from plugins. The running data plugins' folders are now readable, read only, through the card root (`core.files` `stat`/`list` and picture preparation, with the file permission as before); code plugins' folders stay hidden and nothing under `plugins/` can be changed.
