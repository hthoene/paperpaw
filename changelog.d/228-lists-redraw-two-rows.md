### Fixed

- **Lists on interface 7: a step of the focus redraws its two rows, not the screen** (#228). The table of contents, the settings and every other list of a native plugin asked for the whole screen at each step down or up; now they ask for the row the focus leaves and the row it reaches, as Beta 3 did, and the whole screen only where the step turns the page.
