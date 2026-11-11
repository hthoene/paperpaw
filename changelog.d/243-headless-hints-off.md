### Fixed

- **The emulator's headless tests and the screen tools run without the first-time hints again** (#243). With the help plugin native, a fresh library showed the hints over the first screen, and they took the next button (power, the menu), as on interface 6; the tests and tools set `help.show=false` again, as they did then.
