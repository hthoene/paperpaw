### Fixed

- **The native Wi-Fi plugin can be made** (#244). The host refused a registration for `event.wifi_scanned` (92), `event.network_wanted` (94) and `event.network_idle` (95), so the plugin failed at `make` with Factory(-1). They register now, with `event.plugins_requested` (77) and `event.plugin_answered` (78) of the registry; nothing raises 92, 94 and 95 until the radio's own events are wired.
