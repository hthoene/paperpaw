### Changed

- **A plugin reads only the events it handles** (#228, draft 27). `Plugin::event`
  receives `events::Incoming` (the event framed, its body not read) instead of a decoded
  `Event`. `decode_as::<events::kinds::Tick>()?` reads one kind and links only its
  decoder (`Ok(None)` for any other, body unread), `is::<K>()` asks without reading,
  and `decode()` reads any kind as `Event::decode` does (which stays). The 18
  first-party native plugins read their kinds only; `paperpaw_ui_kit::read_event!`
  lists them. End of book 91,716 → 84,704 bytes, Battery 82,944 → 76,468, Profiles
  123,928 → 116,900, Keys 129,436 → 121,636, Settings 156,532 → 149,872, Stats
  59,584 → 55,600. A malformed body of a kind a plugin does not read is no longer an
  error of that callback. `tools/plugin-symbols.sh --events` proves which decoders a
  plugin links, fails on a stripped file, and `tools/flash-gate.sh --plugin-elf` runs it.
