### Added

- Interface 7 host slot 42 `canvas.describe` (`docs/abi/screens-input.md` section 6.1): a native
  screen tells its parts (role, label, area, focused, on, disabled) to automation, reading aloud
  and tests, as a legacy screen does; the host also tells every native `text.draw` as text while
  it asks. Nothing is kept or allocated where nobody asks. SDK: `native::canvas::Canvas::describe_part`
  with `PartRole` and `PartFlags` (#243).
