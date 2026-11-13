### Fixed

- **A choice offered for a native plugin's setting waits for it** (#228, #244). An interface-6 plugin's `add_choice` for a setting a native plugin declares (the vocabulary's sleep card for native sleep's `sleep.show`) was dropped as "no such setting", since native plugins declare their settings after the kernel is built. Until the start such a choice waits and is added when the setting is published; one for a setting nobody declared is dropped at the start, with its log line.
