### Changed

- **`core.net` `serve` works from any callback of a plugin that has a screen showing** (#228). Outside a screen's own callback (an `event.wifi_changed`, a service event) the server is tied to the plugin's topmost screen, if it is showing and not covered, and stops with it as before. Without such a screen, and in a background work run, `serve` still answers `ERR_STATE`. A callback of a covered own screen gets its server on the topmost screen too; when the work scheduler cannot be asked, the call fails closed with `ERR_STATE`.
