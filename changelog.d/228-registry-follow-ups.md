### Fixed

- **The registry lists every key-binding layer and walks slot offers without allocating** (#228). `core.registry` op 13 with an empty scope lists each layer that has a binding (not only `global`, the slots and the screens), and ops 10 and 9 read the slot offers through `Env::each_slot_screen`, so neither allocates.
