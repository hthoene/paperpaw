### Fixed

- **The widget pack no longer fails to start with `NoSpace`** (#228). The index budget for declarations a plugin borrows from its image is 12 KiB (was 8 KiB): the pack declares 407 entries in 65 batches, and the batch and name vectors double, so the 65th batch could not be reserved. Four `native_image_declarations_tests` that failed with `Factory(-3)` pass again.
