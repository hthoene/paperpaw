### Added

- **Native plugins can subscribe to the hint events 70-74** (#228): `plugins_changed`, `wifi_changed`, `power_changed`, `status_changed` and `charge_changed`. A hint is one pending bit in the plugin's queue, so several raised before it is told are one event, and its record is built from the state at the time it is told. They are told between callbacks with the setting changes, a few to a plugin in a pass.
