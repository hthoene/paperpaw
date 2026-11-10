### Added

- **`lookup.spec` is admitted** (#228, `docs/abi/document.md` section 7). A native plugin may declare up to 8 lookup providers (a ninth is `ERR_INVALID` for the batch) and the device holds 32 (the batch that would pass it is `ERR_FULL`); a name is unique within its plugin (`ERR_INVALID`), the same name in two plugins is allowed. The declarations stay in the declaration store with their plugin, in load order, for the lookup service once its host is wired.
