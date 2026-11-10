### Changed

- **The desktop shell and the emulator run the first-party interface-7 plugins** (#228). On Linux x86-64 they register `NATIVE_PLUGINS` through `paperpaw-app/native-linked` with the desktop's `core.ui` and `core.device` services, and the interface-6 form of each ported plugin is no longer installed. Plugins that need services the desktop does not wire yet stay unregistered and are said on stderr.
