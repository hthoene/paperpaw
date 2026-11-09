### Changed

- **News on interface 7, fourth slice** (#228). The feeds are fetched one after the other over the network service, each answer saved to its own file (at most 4 MiB) and made into its page; a feed that fails does not stop the others and a feed removed meanwhile is left out. The action *Fetch the news* declares the background work for when it is cheap, as before. The books themselves are put in the library with the next slice, so the plugin is not yet offered; it still runs as before until the native plugin takes over.
