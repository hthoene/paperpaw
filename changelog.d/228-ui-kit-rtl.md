### Fixed

- **Right-to-left screens on interface 7** (#228). The plugin ui-kit knows the direction of the interface
  language (from `core.language`) and mirrors what it draws, as Beta 3 did for Arabic: the header with its
  back arrow and pager, list rows (title at the right, value at the left), stepper rows and notes, the
  sheet's pager, dialogs (buttons from the right, left and right swapped) and the chips. Screens that draw
  parts of their own take the kit's `Style::flip` helpers; until they do, those parts stay left to right.
