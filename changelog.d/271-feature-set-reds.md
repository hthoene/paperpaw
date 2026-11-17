### Changed

- Tests only: the fonts-default tests check the lean fonts build (the X4 firmware's font set, `PAPERPAW_FONT_LEAN=1`) by what it really has, the UI font and the icons, instead of Literata, and CI now runs them in its feature test step. `a_widget_placed_twice_has_two_looks` uses a widget of the native home, so it passes without `native-linked-store` too.
