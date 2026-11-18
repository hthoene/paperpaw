### Added

- **Translation on interface 7, second slice** (#228). Asking DeepL over the network service and keeping the API key with the secrets are ready on the new interface: the request goes out as before (the key only in the `Authorization` header, texts up to 3,000 characters whole, longer forms sent from a file), the answer is read in pieces up to 24 KiB, and a host that cannot save passwords is told apart so the key can still be used once. The panel itself stays on the old interface for now.
