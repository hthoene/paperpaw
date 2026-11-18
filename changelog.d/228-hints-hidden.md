### Added
- A native screen can be declared with its row of button hints hidden (`screen.spec.flags` bit 8 `hints_hidden`, `ScreenDecl::hints_hidden()`), as a code plugin did with `set_screen_hints(.., Hidden)`: the sleep screen shows no hints. Declared only; `command.screen_policy` cannot change it.
