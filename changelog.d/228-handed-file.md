### Added

- **The host knows which file a reader's hand-over names** (#228). A reader opened with a hand-over that carries `root` (`0` or `2`) and `path` can be asked, by the host, for that file and for the plugin that made the hand-over, which stays known after the maker stops. The document service uses it for `doc_open` root 5; until it is wired, that root still answers unsupported.
