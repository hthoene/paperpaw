### Fixed

- **Native plugins hear the radio's events** (#244). `event.wifi_scanned` (92, with the number of networks found) reaches every plugin that subscribed to it; `event.network_wanted` (94) and `event.network_idle` (95) reach the network manager (`core.network-manager`) alone, and nobody where none is chosen. Each is a coalesced hint.
