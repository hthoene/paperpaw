### Added

- **A part of a picture can be drawn: `canvas.draw_image_part`** (#312, #92, #369). Interface 7
  has one more host slot, 44: it draws a rectangle of a decoded picture scaled into a rectangle
  and reads only that part from the card (SDK: `Canvas::draw_image_part` with `ImagePart`). A
  format can also answer a picture as a part of a cache file (`fmt.image.part`, stage
  `core.format` revision 1), which the host draws for every reader.
  Appended to interface 7; plugins built before run unchanged.
- **Comics panel by panel write no copies to the card** (#312, #369). A panel is now a part of the
  page's one decoded picture, drawn enlarged by the host; before, every panel shown was written
  as an enlarged picture of the page's size (about 384 KB a panel on the X4). Turning to the next
  panel is quicker and wears the card less.
