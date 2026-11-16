### Fixed

- **The X4 Pro and the X4 Classic image has one data segment again** (#275). The plugin list after the application descriptor is padded so that no gap is left before the read-only data, and the link fails if one is. With the gap the image had two data segments, of which the bootloader maps the last only, and the firmware read its own plugin list from the wrong flash page. `paperpaw-sign check-image` and the new `check-app` fail an image with two, and CI runs the second on every device's image. The X4 Pro is held to its own slot, not CrossPoint's (Decision 184).
