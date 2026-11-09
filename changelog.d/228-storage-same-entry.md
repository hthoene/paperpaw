### Added

- **`Storage::same_entry`** (#228): whether two paths name the same entry however each is spelt (`None` where the storage cannot tell, the default). The memory card answers by the entries' clusters, the mounted storage asks the side both paths are on. `core.files` uses it to refuse an alias of a protected folder by identity, not spelling.
