### Changed

- **The firmware is 100 to 146 KB smaller** (#258). The parts that run seldom (the app's pages for settings, store, Wi-Fi, Bluetooth and updates, the built-in plugins behind them) are now built for size, as the kernel already was; opening a book, turning a page, drawing and reading the card are built as before. The released image no longer holds the paths of the build machine: a crash report names `crates/…/file.rs` and the line as before. `shells/xteink/size-budget.txt` holds each device's image size for a check in CI.
