### Changed

- **The X4's nested-callback stack gate is open by default** (#250). `stage.run`, `ui.compose`, the widget ops 7 and 12-15 and op 14 now run another plugin's callback inside the caller's on the X4 too, so a Home page with widgets of other plugins (battery, progress, news ...), the status items and the aids' text rules work on the X4 as on the emulator. Measured on the real X4 (test image, gate open): lowest stack left 14,720 B in the main menu, 15,012 B on Home, Settings and the reader (limit 8,888 B). The core feature `x4-nested` is replaced by `x4-gate-closed`, which closes the gate again; off by default. The X3's firmware builds with it closed until an X3 has measured the gate.

### Fixed

- **The *Pictures* reading aid (caption or hide) now takes effect on the X4** (#250, #257). With the gate closed the reader never asked the aids for their rules, so a book's pictures stayed whatever the setting said; with *Caption* a picture is now a centred `[Picture: description]` line, with *Hide* it is left out, as in Beta 3.
- **The reading settings' pages show the sample text above their rows on the X4** (the *Preview*, as in Beta 3).
