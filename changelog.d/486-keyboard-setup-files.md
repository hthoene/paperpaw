### Fixed

- **Holding Back finishes a password too** (#486). While a password was
  typed, holding Back did nothing; it now keeps the text and closes the
  keyboard, as it does for any other text.
- **A text that could not be passed on stays in the keyboard** (#486).
  When the text could not be handed over (for example a note of more than
  2 KiB on the X4), the keyboard emptied the field and stayed open without a
  word. Now the text stays, and a notice says it was not passed on: it can
  be shortened and sent again.
- **Passwords leave nothing behind in memory** (#486). What was typed and
  deleted again, and the text when the keyboard could not open, is now
  cleared from memory as well.
- **Typing redraws only the text field** (#486). A key that only changes the
  text redraws the field instead of the whole screen, and Delete on an empty
  field redraws nothing.
- **The setup guide lets other buttons work** (#486). While a number was
  being changed, Power and the menu button did nothing; swipes and long
  presses on the guide did nothing either. They now work as on any screen.
- **Moving the focus redraws only what changed** (#486). In the setup guide,
  the library and Files, a focus step within the page redraws the two places
  it leaves and reaches, not the whole screen.
- **Files moves through a folder without reading the memory card** (#486).
  Each arrow key asked the memory card for its free space and the folder
  again; a step within the page shown now reads nothing, and finding out
  whether an entry is a folder no longer lists the whole folder.
- **Long names in Files** (#486). Renaming to a long name with letters such
  as ä or 語 failed without a word; names as long as the memory card takes
  now work, and a longer one says it is too long. On a memory card that
  cannot be read, finding a free name no longer tries thousands of names
  before it gives up.
- **The library says when marking a book failed** (#486). When a book could
  not be marked finished or unread, or put among the favourites or taken
  out, nothing was said; a notice now says so.
