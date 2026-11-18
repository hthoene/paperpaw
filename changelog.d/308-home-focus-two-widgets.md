### Fixed

- **Moving the focus on Home is faster on the X4 again** (#308). A frame is
  drawn in bands of rows, and every text, line and cover of a widget was sent
  to the host in each band it touched, also where it lay in another one. Now
  each is sent only in the band it lies in: a move between the Continue card
  and the shelf draws each cover once and needs about a third fewer host calls.
  The raw key before a focus press no longer makes Home ask the library for
  its books again at the next draw.
