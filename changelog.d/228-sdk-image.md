### Added

- **Pictures in the native SDK** (#228). `native::image`: `ImageId`, `ImageInfo::read`
  (`image.info`), `Canvas::draw_image` with `ImageStyle` (`canvas.draw_image`),
  `CommandSender::image_prepare` (`PrepareImage`, `PrepareRoot`) and `image_release`,
  and `Event::ImageReady` (`ImageReady`, `ImageStatus`) with
  `EventKind::ImageReady`.
