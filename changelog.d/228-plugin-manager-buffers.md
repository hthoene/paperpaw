### Changed

- **Plugin manager (interface 7): smaller reads at the X4 start and in the list** (#248). The registry and device queries take their request from a 256-byte stack array instead of a 4 KiB heap block, the plugin rows are reserved for the registry's count at once, and the start's notices free their read buffer before they ask for a name.
