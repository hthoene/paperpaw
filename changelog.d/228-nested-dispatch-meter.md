### Fixed

- A plugin's callback reached from inside another plugin's (a stage subscriber, a compose child's draw) now runs on the usage meter as itself, so its time, storage and network are charged to it and the secrets and Bluetooth views follow, and the caller runs again after it.
