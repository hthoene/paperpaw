### Changed

- **Reader: the button hints show after a press, as in Beta 3** (#448). On the X4 the row *Back · Select · ‹ ›* shows over the bottom of the page for a few seconds after a button is pressed and then goes again; the page keeps the whole display. For plugin authors: interface 7 draft 38 adds `ScreenDecl::hints_on_press` (`screen.spec.flags` bit 13; `docs/abi/ui.md` 1.2); plugins built for draft 37 run unchanged once built again.
