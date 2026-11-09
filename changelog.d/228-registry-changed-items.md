### Added

- **`event.registry_changed` bit 0 for pushed item states** (#228). When a plugin's `command.set_item_state` batch changes what the host holds (a new, changed or cleared state), or an unloaded plugin's states end, the host raises bit 0 of `event.registry_changed` once per drain; pending bits are OR-ed per plugin until delivered, so none is lost. The same state pushed again raises nothing. The room a callback reserves for item states is now given back by a guard on every path, a failed or refused callback included.
