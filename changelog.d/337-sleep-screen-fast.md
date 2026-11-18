### Fixed

- **The sleep screen no longer reads its picture back from the card** (#337).
  Making a gray picture ready (the sleeping cat, the logo, a cover, a picture
  of a folder) read the finished picture again row by row from the card for its
  brightness map, about 290 card calls for the cat. The map is now summed while
  the picture is decoded, so the sleep screen asks the card about 23 times
  before its first frame instead of 293.
