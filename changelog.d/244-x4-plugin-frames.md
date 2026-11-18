### Fixed

- The Mobipocket format, Mail and the online catalogues (OPDS) pack as plugin files for the X4 again: each had a function with a stack frame of 2 KiB or more, which the X4 refuses. A picture's header is read into memory asked for, not onto the stack, and the sorts of mail rows and catalogue entries use the shared sorter (the same order).
