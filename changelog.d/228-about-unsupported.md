### Fixed

- **The native About page opens on a host without firmware facts** (#243, #228). Where the host has no backend for the device's firmware, built-in plugins or native facts (the desktop shell answers *unsupported*), the page now opens without those rows instead of failing its first callback; the lines other plugins add are likewise optional.
