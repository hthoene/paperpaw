### Changed

- **Reading a setting is cheaper for native plugins** (#250). For every setting a plugin read, the host walked all settings kept as they are to tell whether the one asked for is a secret; it now finds it by the settings' index. The reader reads its settings many times per page turn, so this was a large part of a page turn on the X4.
