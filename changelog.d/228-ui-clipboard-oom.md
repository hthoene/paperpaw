### Changed
- The clipboard (`core.ui` op 18) reserves room for a text written in parts a few times (doubling up to the 4,096-byte cap), not once a part; the copy out to the system clipboard is its own step, tested for the log line when it finds no memory.
