### Added

- **A screen is told when a draw only counts its pictures** (#250). To decide whether a page is shown in grays the host draws the screen once aside. That draw's frame now carries `id` bit 1 (`DrawFrame::pictures_only()`): a native screen may leave out its text and status lines there, so the reader's sixth draw of a page turn need not cost a whole draw. A screen that ignores the bit draws and is counted as before.
