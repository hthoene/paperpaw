### Added
- Questions (interface 7, `core.ui` op 8) now open their dialog in the slot of their kind once the asking callback returned, are cancelled as `not_started` when that callback failed or no screen serves the slot, and their answers reach the asker as `event.answered` (82), one callback each.
