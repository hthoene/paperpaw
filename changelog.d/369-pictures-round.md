### Added

- **Plugins can shape how pictures look: the picture stage `core.picture`** (#82, #369). A plugin
  that subscribes to the new stage (at most two on a device) is asked once for each picture the
  host prepares, with where it is shown, its size and a histogram of its grays, and may answer a
  tone curve of 256 grays and whether it is drawn in dots or in grays. It sees no pixels and is
  never asked at a draw, so page turns cost nothing more. The SDK has it as
  `native::image::picture` (`Asked`, `Tone`, `Place`). This first host asks for the pictures a
  plugin prepares with `image_prepare`; pictures of books, covers and thumbnails follow
  (`docs/abi/document.md` 5).
- **A part of a picture can be drawn: `canvas.draw_image_part`** (#312, #92, #369). Interface 7
  has one more host slot, 44: it draws a rectangle of a decoded picture scaled into a rectangle
  and reads only that part from the card (SDK: `Canvas::draw_image_part` with `ImagePart`). A
  format can also answer a picture as a part of a cache file (`fmt.image.part`, stage
  `core.format` revision 1), which the host draws for every reader.
- **Comics panel by panel write no copies to the card** (#312, #369). A panel is now a part of the
  page's one decoded picture, drawn enlarged by the host; before, every panel shown was written
  as an enlarged picture of the page's size (about 384 KB a panel on the X4). Turning to the next
  panel is quicker and wears the card less.
- **Turn wide pictures** (#92, #369). A new reader setting (Reading behaviour, off to begin with):
  a wide picture shown full screen (hold Select on a page with a picture) is turned a quarter so
  that it fills a tall screen. The host turns the picture once, when it is asked for, not at every
  draw: `Documents::ask_image_turned` (op 16 `flags` bit 0) and `PrepareImage::turned`
  (`image_prepare` bit 3) in the SDK.
- Plugins built before these additions run unchanged: everything is appended to interface 7
  (a host slot, optional fields, flag bits, a stage), nothing changed or removed.
