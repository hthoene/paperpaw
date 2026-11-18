### Changed

- **Built-in key maps are no longer copied onto the heap** (#248). Presets, plugins' key maps and the device's key map are kept in the program, so their layer names, button names and action ids are now borrowed instead of parsed into heap copies. The user's and the shell's bindings are still copied.
