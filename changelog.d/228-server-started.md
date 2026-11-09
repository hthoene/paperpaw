### Changed
- `core.net` servers: the plugin is told `event.server` `started` (with the address) only when the shell reports that the listener opened (`NetReports::server_started`), not when its `listen` returns. A listener that cannot open is reported with `NetReports::server_failed`: the plugin gets `failed` with reason `refused` and the server slot is free again.
