### Added

- **UI kit: the model of the status items editor** (#228). `paperpaw_ui_kit::status_items::Chooser`
  keeps what a setting that lists status items or lines holds and answers the rows to draw, what a
  tap, a drag or the buttons (pick up, move, put down) make of it; items of plugins that are not
  there stay where they were. It is the behaviour of the interface-6 items screen, checked against
  it on every small setting, and refuses cleanly when memory is short. Plugins name their own
  screen built on it as the editor of such a setting.
