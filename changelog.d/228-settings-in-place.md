### Changed

- **Settings of plugins loaded from the card are described on demand** (#248). The kernel
  kept a full copy of every setting such a plugin declares (titles, help texts, choice lists,
  ids) on the heap for as long as the plugin was installed. It now keeps each setting's
  value, default and a few flags, and reads the rest from the plugin's own data when a
  screen, a search or a change asks for it; what a round of the kernel read is let go at its
  end. Plugins built into the firmware keep their tables and pay nothing. A plugin may
  describe at most 16,384 settings this way.
- **At most 16,384 settings built at run time** (#248), half of the 32,768 before: the
  places of settings now also tell those a plugin describes. The next one is refused as
  before ("no place left for another setting"). Inside the SDK only: no change to
  `abi.def` or `abi.lock`.
- **No room is told as no room** (#248). Where the heap has no room to read a setting's
  description, `command.set_setting` and the `core.registry` setting queries answer
  `ERR_NO_MEMORY` instead of calling the setting invalid or not found, the change is refused
  with a warning and the value stays as it was. Whether a setting is guarded or a secret
  never depends on free memory.
