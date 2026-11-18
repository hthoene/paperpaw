### Changed

- **A focus move on Home no longer reads the covers from the card** (#329).
  Every frame that drew a cover's rows again read its whole decoded file
  (57 KB), scaled and dithered it: some 100 ms a cover on the X4, at every
  move of the focus, three covers on a Home with a book on the card and two
  on the shelf. Now what a draw put into a black and white frame is kept
  while Home is shown, a bit a pixel (3 KB a cover, 12 KB at most, small
  pictures only), and the same draw again takes its rows from there: the same
  pixels, no card. The room is taken between frames, and a screen opened over
  Home gives it back before it takes its own.
