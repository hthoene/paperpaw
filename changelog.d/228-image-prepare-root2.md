### Added

- **`image_prepare` reads the plugin's install folder, and a missing file is status 6** (#228). Root 2 (the plugin's own install folder, read only, no feature) is accepted and resolves like `core.files` does (`plugins/<plugin>/<path>`); a built-in plugin's folder is empty, so its paths are not found. `event.image_ready.status` 6 (`not found`) now tells a missing source file on every root, where a missing file was status 4 (card failure) before; other card failures stay 4.
