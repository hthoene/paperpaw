### Fixed

- Native plugins hear the device's start at once: an event raised after a call's pump (the kernel's
  `started`, raised as the commands of the start run) now asks for the next call now, not at the
  next deadline, so *Open the last book at the start* no longer waits for the first key press (#243).
