### Added

- A native plugin's `help.hint` (a hint for any screen, own or another's) and
  `input.spec` (an input or gesture of its own) records are admitted at
  install and kept in the declaration store, removed with their owner. A hint
  names a screen by full id, the inputs it is for (any, touch, buttons), an
  optional zone that lies on the screen (thousandths, a sum that overflows is
  refused) and the catalog key of its text; a plugin may not repeat a hint
  (same screen, input and text) or an input name. `core.registry` op 14
  (`inputs`) lists the declared inputs and gestures the device meets, after
  the device's own.
