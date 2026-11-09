### Changed

- **Interface 7 draft 30: screens know what the user used last, typed text
  stays private, and pictures too large for good are told apart** (#228).
  `Event::ScreenShown` says whether buttons, touch or a pen were used last, so
  a screen shows a focus at once only for buttons. Typed text from a keyboard
  or a remote (`Event::TextTyped`) never shows in debug output. The image
  decoders answer `ImageError::TooLarge` for a picture that can never fit, and
  keep `OutOfMemory` for memory that is short now, so a reader keeps the
  alternative text for good only for the first. `ImageFile::try_open` and
  `Decoded::try_open` open a decoded picture without aborting when memory is
  short. Lookup providers that went online tell the reader when their answer
  is ready (`Event::LookupReady`, `CommandSender::lookup_ready`), and
  `Font::covers` says whether a font has the glyphs for a text. Each stage
  codec has a `ReplyWriter` and an `InputWriter` that write a batch record by
  record, so a format plugin answers 64 rows without a slice of them on the
  stack.
