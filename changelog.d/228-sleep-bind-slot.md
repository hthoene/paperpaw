### Changed

- **Another plugin's sleep mode shows through the slot `core.sleep`** (#228). When `sleep.show` names a
  screen another plugin offers for the sleep slot (reading-goal's), `sleep` binds the slot to it; for its own modes,
  an empty value or a mode nobody offers any more it binds the slot back to the sleep screen, which falls back to the
  cover or the cat as before. Binding the slot needs the optional `uses.registry` permission.
