### Fixed

- **`MemoryStorage::list` no longer aborts when memory runs out** (#228). The SDK's in-memory storage for plugin tests answers `StorageError::OutOfMemory` when the heap refuses, before anything is listed, and lists a folder without subfolders without allocating, so out-of-memory tests fail on the code they test.
