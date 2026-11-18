### Added

- **Translation on interface 7, first slice** (#228). The translation panel is declared to the host as interface 7 asks, and the parts of translating that need no host — which language DeepL is asked for, the request with the key kept in a buffer that wipes itself, and reading the translation from the answer — now run without panics, report out-of-memory instead of aborting, and read answers up to 24 KiB. Asking DeepL and showing the panel stay on the old interface until the network, secrets and screen services are there.
