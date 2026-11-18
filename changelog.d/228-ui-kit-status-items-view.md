### Added

- **UI kit: the screen of the status items editor** (#228). `paperpaw_ui_kit::status_items::view`
  draws the editor on top of the chooser: the shown items in order, then the others, a hint line and
  the item being moved. Buttons work as on interface 6 (Menu picks up, direction and page buttons
  move, Confirm, Back or Menu put down); on touch, a long press picks an item up and a tap puts it
  down, since the kit has no drag. Two new texts, `core.status-items.hint` and
  `core.status-items.moving`, in English and German.
