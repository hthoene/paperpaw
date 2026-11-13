### Changed

- Emulator: the secrets sealer (1.0 KiB) and the lists of services and features offered to the linked plugins (0.6 KiB) are in static memory, not on the heap, so the start holds 1.6 KiB less (#248, P3).
