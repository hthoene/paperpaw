### Added

- **Inked text in the native SDK** (#228). `Font::draw_inked` draws text through host slot 41 with an `Ink`: glyphs widened by `extra` pixels, an optional 16-step tone curve and the `sharp` mode, as the per-book font weight, smooth-text and text-contrast settings of Beta 3 need.

### Changed

- **Bold runs draw through the regular font** (#228). `Paint::Bold` in the UI kit widens the regular font by `max(1, size / 20)` pixels, as Beta 3 did, so a plugin no longer acquires a bold font: `FontRole::UiBold` and `FontRole::Mono` are removed (the host never had `role:ui-bold` or `role:mono`), and `replay_with` and `replay_all` give way to `replay` and `replay_icons`.
