### Changed

- **Native plugin screens** (#228). The X4 firmware (with interface-7 plugins, `ppn5`) and the desktop emulator (developer switch) now wire the `core.ui` services beside `text.hyphenate`, sized for the device by its id, so plugins' screen-stack services are served there. Android runs no code plugins and needs none.
