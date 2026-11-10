### Changed

- **The interface-6 Wi-Fi plugin is gone; the native one is listed in the app** (#228). `paperpaw-plugin-wifi` is now the native interface-7 plugin only (the old policy, pages, phone setup and their suites are removed, no shim) and `NATIVE_PLUGINS` carries it. Admitting it on a device waits for the raised callback cap (#871).
