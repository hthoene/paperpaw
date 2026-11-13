### Added

- **Theming (#114, build-out): the interface kit's shared parts can be styled by a theme.** New appearance settings, each leaving the look as it is until a theme sets it: `core.ui-shadow` (a hard shadow under dialogs and chips; the bottom sheet has no free edge and takes none), `core.ui-frame` (the plate of sheets, dialogs and chips: none, line, card, dark), `core.ui-density` (compact, regular, roomy margins and gaps), `core.ui-button` (outlined, filled, flat buttons) and the light bar for `core.ui-focus = core.focus.soft` on panels of more than two grays (a frame on two). A theme also colours the panel, the chosen row's bar, filled buttons, outlines, the chosen chip and the dark header bar through new names in `core.ui-palette`. The SDK gains the setting constants only (`api.txt` and `shape.txt`, lines added, none changed). The sample theme `theme-slate` sets them all. Texts in English and German.
- **ui-kit: `StyleCache`.** A screen can keep its `Style` and clear it on `setting_changed` and when it is shown; nothing in the kit holds one for a screen (a read is 12 host calls, about one per screen rebuild, measured on the X4 profile).

### Changed

- **A dark frame on a dark page is a card** (it was the page's own paper, so the card was not to be seen).
