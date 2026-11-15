### Added

- **Choose a font for each widget** (#262): every widget on the home and sleep screen can use its
  own font, including fonts from your SD card. Each widget's settings have *Font* (as all
  widgets, as the menus, as the book, or a font family); *Font of widgets* in the home screen's
  advanced settings sets it for all widgets that have none of their own. Text larger than a card
  font has (on the X4 they have 20 to 36 px, so the large digits of a clock) stays in the menus'
  font, and at most three families beside the menus' and the book's are open for widgets at once.
- **Plugins: the font key `widget:<widget id>`** (#262, `docs/abi/screens-input.md` section 4): the
  host gives every widget a plugin declares, that has a settings group of its id, the setting
  `<widget id>.font`, and resolves the key to the font chosen. A widget drawn with the UI kit's
  `widget::Cx` is set in it without a change (`widget::cx::widget_font` for one that is not). No
  record, slot or lock change.
