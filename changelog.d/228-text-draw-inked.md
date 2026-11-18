### Added

- **Inked text for native plugins** (#228). Host slot 41 `text.draw_inked` draws text as `text.draw` does, with glyphs widened by up to 16 pixels (more is clamped) and an optional 16-step tone curve (or without grays at the screen's own threshold). This gives readers the per-book font weight, smooth-text and text-contrast settings, and lets widgets draw bold runs as Beta 3 did. Hosts answer it as unsupported until it is wired.
