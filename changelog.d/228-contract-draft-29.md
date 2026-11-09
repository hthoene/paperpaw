### Changed

- **Interface 7 draft 29: widgets keep their least size, screens label their
  buttons, and the X4 Pro and X4 Classic get their plugin profile** (#228).
  A widget declares the least columns and rows it is shown in and whether it
  stays square (`WidgetDecl::min_cells`, `square`), and home screens read them
  (`RegistryWidget::cells`, `min_cells`, `square`), as interface 6's size hint
  did. A screen relabels or hides the button hints under it
  (`command.set_hint`, interface 6's `Screen::hint`). Setting choices hold up
  to 64 values, the environment snapshot carries the user's UTC offset, font
  faces carry their weight, and `Native::minor` says which interface-7 minor
  the device loads, so a store can tell which plugins run on it.
- Interface 7 Xtensa profile `esp32s3-xtensa-lx7-ilp32-windowed-le`, target 9
  of format 5, for the X4 Pro and X4 Classic (`abi/entry.md`), and the S3's
  stack band design (`abi/heap-word.md` section 6) (#228).
- Contract text: books open at a place, block direction, writing mode,
  language and annotations in documents, and library paths that never reach
  the plugins' or the kernel's folders (#228).
