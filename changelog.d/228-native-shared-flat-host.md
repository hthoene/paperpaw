### Changed

- Every interface-7 plugin's dispatch shares one set of the kernel's handles (meter, stack rows,
  declaration directory …) instead of a copy each: 40 bytes less per native plugin on the
  desktop, about 0.9 KiB with 24 linked plugins (#248).
