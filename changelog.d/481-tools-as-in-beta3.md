### Fixed

- **Wi-Fi keeps the focus where you were** (#481). Joining a network put the
  focus back on the first network of the list, and Back from the networks or
  from the phone setup put it on the first row of the Wi-Fi page. As in
  Beta 3, the focus stays on the network chosen and comes back to the row the
  page was left from.
- **A Wi-Fi password can be typed on the phone again** (#481). The keyboard
  offered *Phone* for every text but a password, the one it is most handy
  for. As in Beta 3, a password can be typed on the phone too: masked there,
  and what was typed on the reader is not sent along.
- **The key bar names only what a button does** (#481). The Wi-Fi transfer,
  the phone's typing page and the battery page showed *Select* and the arrows,
  which do nothing there. As in Beta 3, only *Back* shows, and *Select* says
  *Without a router* where it opens the reader's own network.
- **The device check shows its steps** (#481). The device check (diagnostics)
  stayed an empty page: it asked the device about itself with too small a
  reply buffer, which the host refuses.
- **Long texts on empty pages show whole** (#481). The empty *Passwords and
  keys* page, the statistics' empty book list and the help of *Sync with
  KOReader* were cut to a line or four lines with "…"; they are wrapped whole
  again.
