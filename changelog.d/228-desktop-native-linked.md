### Added

- **The desktop shell and the emulator can run the first-party interface-7 plugins** (#228, `--features native-linked`, Linux x86-64). The feature registers `NATIVE_PLUGINS` with the desktop's `core.ui` and `core.device` services and drops the interface-6 form of each ported plugin; plugins that need services the desktop does not wire yet stay unregistered and are said on stderr. It is off by default until those services are wired.
