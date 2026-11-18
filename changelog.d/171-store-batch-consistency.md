### Changed

- The store plugin update workflow can publish a batch of interface-7 code plugins from the current base against the released firmware (`plugins`: ids, `changed` or `all`), and `tools/plugin-files/store_consistency.py` refuses a changed plugin without a version bump and a plugin or file whose ABI the released firmware would refuse. It publishes nothing; how it is run is in `docs/releases.md`.
