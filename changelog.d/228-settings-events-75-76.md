### Changed

- **Settings: constructed in the host with all its event subscriptions** (#228). With the host admitting `registry_changed` and `preview_changed`, the native Settings plugin is now built in the kernel's registered scope by a test (it was refused as unsupported before).
