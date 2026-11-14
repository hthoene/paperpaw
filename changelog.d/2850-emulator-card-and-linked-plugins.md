### Fixed

- **The emulator registers its built-in plugins beside an installed interface-7 card plugin** (#2850). With `--abi7-dev-loader --trust-code` and one such plugin installed, every built-in plugin failed with status -6, because the card plugins' loader and the built-in registration each made their own table of handles and dispatch budget, and a kernel takes one of each. They now share one per kernel.
