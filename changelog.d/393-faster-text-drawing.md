### Changed

- **Text draws faster** (#393). A word's pixels are now handed to the screen a row at a
  time and gathered eight to a byte, not one by one. A text draw also
  checks its font once, not twice. On the sample page a word takes about
  25 % fewer instructions to draw.
  The look of the text does not change.
