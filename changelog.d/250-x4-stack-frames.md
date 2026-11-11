### Fixed

- **The X4's start and main loop need less stack** (#250). A shell can build the kernel on the heap (`KernelBuilder::build_boxed`), so no copy of it (about 6 KB there) is on the stack while the start reads the card. The bounded steps of a plugin install that a stop asks for run at the end of the kernel's pass, before its effects, instead of under the command that stopped it. A signed file's check keeps its state on the heap, and a few dispatch steps run in calls of their own.
