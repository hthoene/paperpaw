### Changed

- Kernel: `KernelBuilder::items(manifest, |items| ..)` takes contributions as a closure, so a test fixture no longer needs a type with `impl Plugin`; `Items` is the name its calls go through (#244).
