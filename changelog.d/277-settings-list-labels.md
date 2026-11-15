### Fixed

- **List values in Settings** (#277). Every list setting, including keyboard layouts, resolves catalogue keys to translated names and shows “None” for an empty list in English and German. Items without a catalogue entry keep their internal identifier, so some widget and status lists still mix names and identifiers: sleep widgets can show “home.date, Continue reading”, and a Home footer can show “Time, reader.status.page”.
