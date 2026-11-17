### Changed

- **The kernel's and the X4's own texts are carried without their keys** (#306). The kernel's catalogs (menus, hints, settings, host prompts) and the X4 shell's are built into the program the way the linked plugins' are: a hash of each key and the messages, English and German in full, instead of the `.ini` sources with every key spelled out. About 30 KB less flash on the X4 family; what the reader sees is unchanged. `KernelBuilder::built_messages` takes the tables; a program built without `paperpaw-app` (the kernel's own tests, the `testing` feature) still carries the sources.
