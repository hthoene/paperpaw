### Fixed

- **The widget pack no longer fails to start with `NoSpace`** (#228). The batch and name vectors that index the declarations a plugin borrows from its image now reserve exactly what is needed instead of doubling: the pack (407 entries in 65 batches, 7.4 KiB) did not fit the 8 KiB budget with the doubled 512-name vector. Four `native_image_declarations_tests` that failed with `Factory(-3)` pass again.
