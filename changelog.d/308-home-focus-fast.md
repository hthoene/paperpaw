### Fixed

- **Moving the focus between the widgets on Home is faster on the X4** (#308).
  A press that only moves the focus no longer makes Home ask the library for
  its books again and compose the Continue card and the shelf again at the next
  draw: they are drawn from what was read when Home opened, and only the two
  widgets the focus left and came to are drawn again.
