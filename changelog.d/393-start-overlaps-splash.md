### Changed

- **The X4 starts about a second sooner** (#393). The starting picture's 1.7 s
  refresh is no longer waited for before the plugins are hosted and the
  kernel starts: that work runs while the panel is busy, and only the first
  Home frame waits for it.
