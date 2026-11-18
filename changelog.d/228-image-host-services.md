### Added

- **Image services for interface 7** (#228). The host now serves
  `image.info`, `canvas.draw_image`, `image_prepare` and
  `text.hyphenate`: pictures are drawn in bands, a failed read or a short
  cache file is reported instead of drawing half a picture, and
  prepared images are decoded one request per main-loop turn.
