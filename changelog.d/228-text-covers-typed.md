### Added

- **Glyph coverage and typed text in the native SDK** (#228). `Font::covers(text)`
  wraps host slot 39 `text.covers` (the byte length of the longest start of the
  text the font draws), and `Event::TextTyped { now_ms, text, done }` (text as `TypedText`, whose `Debug` shows
  only its length) decodes
  `event.text_typed` (109), text from a hardware keyboard or a shell's remote
  control, for screens on the `core.keyboard` slot.
