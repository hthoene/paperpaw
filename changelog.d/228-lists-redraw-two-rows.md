### Fixed

- **Lists and the keyboard on interface 7: a step of the focus redraws what it leaves and what it reaches, not the screen** (#228). The table of contents, *Add widget* and every other list of a native plugin, and the on-screen keyboard, asked for the whole screen at each step; now they ask for the row (or key) the focus leaves and the one it reaches, as Beta 3 did, and the whole screen only where the step turns the page.
