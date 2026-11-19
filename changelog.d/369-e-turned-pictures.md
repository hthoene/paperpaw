### Added

- **Turn wide pictures** (#92, #369). A new reader setting (Reading behaviour, off to begin with):
  a wide picture shown full screen (hold Select on a page with a picture) is turned a quarter so
  that it fills a tall screen. The host turns the picture once, when it is asked for, not at every
  draw: `Documents::ask_image_turned` (op 16 `flags` bit 0) and `PrepareImage::turned`
  (`image_prepare` bit 3) in the SDK.
  Appended to interface 7 (flag bits and an optional field); plugins built before run
  unchanged.
