### Added

- **`format.spec` and `reader.spec` are admitted** (#228, `docs/abi/document.md` section 1). A native plugin may declare up to 8 formats (32 on the device) and reading apps; the declarations stay in the declaration store with their plugin. Within a plugin each extension is claimed once (`ERR_INVALID`), a reader's `screen` is one of the plugin's own screens, and a ninth format of a plugin or a thirty-third on the device is `ERR_FULL`. The document service reads them from there once its host is wired.
