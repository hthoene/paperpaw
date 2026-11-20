### Changed

- The firmware is about 90 KB smaller on every device: the built-in fonts (the interface font and the icons) keep their glyph bitmaps deflated in blocks and the glyph cache inflates the block a glyph lies in when it is first drawn (#428). The first drawing of a glyph costs a block's inflating (about 0.3 ms on the X4); a glyph drawn again is found in the cache, which is faster than reading it in place was. Text looks as before, pixel for pixel.
