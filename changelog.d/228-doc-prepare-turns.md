### Fixed

- **A book whose format cannot be called now no longer holds up the others while they prepare** (#228). Where several documents prepare in turns (not the X4), a step that has to wait still passes the turn on, so a book whose format plugin is not started yet does not starve the rest. A kernel test now drives the preparing loop through the kernel's own pump with a format plugin in the instance registry.
