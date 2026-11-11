### Added

- **Native pictures can keep their transparency** (#243). `PrepareImage::see_through()` (`image_prepare` flags bit 2) makes a PNG or GIF in gray with how opaque each pixel is. Drawn, it shows what is below through it, as the Beta 3 sleep picture showed the page. A JPEG has no transparency and is made as before.
