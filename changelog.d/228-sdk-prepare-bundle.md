### Added

- **`PrepareRoot::Bundle`** (#228). `image_prepare` also reads the plugin's own install folder
  (root 2, read only, no feature), for the pictures a plugin ships. A plugin built into the image
  has none there and the host answers not found.
