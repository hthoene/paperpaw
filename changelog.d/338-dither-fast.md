### Changed

- **Pictures are dithered and drawn about three times faster** (#338). The
  error diffusion of a picture (the Atkinson dither of covers, the sleep
  screen's cat, reader pictures) and the storing of its gray rows in the frame
  took some 120 instructions a pixel each; they now take about 40 each: no
  bounds checks or saturation in the diffusion, and the frame packs whole bytes
  of pixels instead of reading and writing each one. The pictures are
  unchanged, pixel for pixel.
