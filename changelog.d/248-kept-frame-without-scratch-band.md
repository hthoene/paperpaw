### Changed

- **A frame on the X4 takes about 9.7 KB less heap while it is drawn** (#248). The firmware keeps the
  bands of a frame packed, to send them to the panel again after the refresh. It packed each band
  into a second band's worth of memory first and held that until the frame was done; a band is now
  counted and then packed straight into memory of its packed size. What the panel is sent is the
  same.
