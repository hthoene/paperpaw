### Fixed

- **Settings > Manage plugins opens in about a third of the time on the X4** (#291). With about 70 plugins the first frame made some 6,200 host calls (4,300 of them text measures: every band of the screen measured the wrapped note of every row, a word at a time). The height of a note is now one measure (it fits a line or it takes two), the heights are kept for the bands of a frame, and a row outside the band being drawn is not laid out: 1,830 calls, 280 measures.
