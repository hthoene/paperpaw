### Fixed

- **A plugin can switch another plugin on or off again** (#277). The command `command.set_plugin_enabled` was refused with "unsupported" by the host, so the store's *Switch on* for a built-in plugin (and the first-run offer) saved nothing. The host now stages it for the next start as the settings editor does; `core` and a plugin switching itself off are refused with `ERR_DENIED` (`docs/abi/registry.md` 4.2).
