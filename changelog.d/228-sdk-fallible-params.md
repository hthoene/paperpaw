### Added

- **`Params::try_set` and `Params::try_clone` report a memory shortage** (#228). They copy with the allocation checked and return `TryReserveError` instead of aborting; on an error the parameters are as they were. Code that builds the parameters of a screen or command on a small heap uses them instead of `set`, `with` and `clone`.
