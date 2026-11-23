### Fixed

- **Plugins that read keys see a page as private only while it is on
  top** (#483). A password page left open further down made the keyboard
  and other key plugins treat every screen above it as private, and only
  that page's plugin could read the keys.
