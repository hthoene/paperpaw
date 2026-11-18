### Added

- **On the X4 Pro and X4 Classic a plugin call is refused where the stack band cannot guard it** (#228). A call that starts at an interrupt level of 1 or more, or on the second core, is not run and is logged once; the band's interrupt cannot fire there.
