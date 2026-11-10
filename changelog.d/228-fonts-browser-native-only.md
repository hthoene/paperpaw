### Changed

- **The fonts browser is an interface-7 plugin only** (#228). The firmware that links native plugins has *More fonts* and *Your own fonts* from the native plugin; the interface-6 plugin is gone. Until a shell activates native plugins (the desktop emulator, Android, web) it has no fonts browser, and the font library address no longer shows in the developer settings there. The names in the list and the sample sentence are not shown in the font's own face yet (the host cannot load a font at run time).
