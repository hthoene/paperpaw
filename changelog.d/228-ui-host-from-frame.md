### Changed

- **The screen stack service reads its state from the running call** (#228). The `core.ui` service no longer needs a separate host object from the shell; one call in `shell_wiring` (`HostWiring::ui`) wires it.
