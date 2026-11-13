### Changed

- **Theming (#114, steps 0 and 1): the interface kit draws its widget frames in the theme's inks, with no pixel changed.** The frames of home-screen widgets (dark, line, card, inverted plate, shadow) took a literal black or white; they now use the theme's text and paper inks, so a dark theme is no longer inverted on widget plates. With the default palette the colours are the same. A test draws every frame in a light and a dark theme and checks that only the theme's inks come out.
