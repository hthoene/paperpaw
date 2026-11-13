### Added

- **Theming (#114, build-out): the interface kit's shared parts can be styled by a theme.** New appearance settings, each leaving the look as it is until a theme sets it: `core.ui-shadow` (a hard shadow under dialogs and chips; the bottom sheet has no free edge and takes none), `core.ui-frame` (the plate of sheets, dialogs and chips: none, line, card, dark), `core.ui-density` (compact, regular, roomy margins and gaps), `core.ui-button` (outlined, filled, flat buttons) and the light bar for `core.ui-focus = core.focus.soft` on panels of more than two grays (a frame on two). A theme also colours the panel, the chosen row's bar, filled buttons, outlines, the chosen chip and the dark header bar through new names in `core.ui-palette`. The SDK gains the setting constants only (`api.txt` and `shape.txt`, lines added, none changed). The sample theme `theme-slate` sets them all. Texts in English and German.
- **ui-kit: `StyleCache`.** A screen can keep its `Style` and clear it on `setting_changed` and when it is shown; nothing in the kit holds one for a screen (a read is 12 host calls, about one per screen rebuild, measured on the X4 profile).

### Changed

- **A dark frame on a dark page is a card** (it was the page's own paper, so the card was not to be seen).
- **Fixed (#114): the strip between a header and the list under it showed the screen's clear colour, not the theme's paper.** In a theme whose paper is not white (or a dark one) the strip was a white band; lists, windows and item lists now paint the page up to the header. With the default look nothing changes.
- **A theme's light `focus`, `control` and `chip` colours are left out on a panel of two grays** (they would draw as paper, an invisible bar or button); the chosen row's frame there is the `outline` colour, else the ink.
- **A card plate gets its edge where the theme coloured `outline`** (a card the colour of the page had no visible edge at its top and left, only the shadow).
- **Content on a plate takes the ink that reads on it** (a light card in a dark theme gets dark text; the dialog's gray text follows a dark plate).
