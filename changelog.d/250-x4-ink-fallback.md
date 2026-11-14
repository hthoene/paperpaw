### Fixed

- **Plugin screens are drawn on the X4 again** (#250). On the X4 the home screen, the menu, the settings, the reader's contents, the store and the battery screen stayed white with the native plugins: the X4's host does not answer `text.ink` yet, and `Font::ink` gave that as an error, which ended the whole draw of every screen that centres text or an icon by its ink. `Font::ink` now gives `None` there, as for text without ink, and the text is placed by the font's metrics.
