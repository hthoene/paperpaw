### Added

- **`PrepareRoot::Bundle`** (#228). `image_prepare` also reads the plugin's own install folder
  (root 2, read only, no feature), for the pictures a plugin ships. A plugin built into the image
  has none there and the host answers `ImageStatus::NotFound`, a new status (`event.image_ready`
  status 6: no such file at that root and path; an older SDK reads it as `Damaged`).
