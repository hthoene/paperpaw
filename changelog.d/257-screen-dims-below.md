### Added

- **Overlays can fade the page behind them again** (#257). A native overlay screen (a menu, a panel) may be declared with `ScreenDecl::dims_below()` (`screen.spec.flags` bit 10): the host washes out what lies below it while it shows, as Beta 3 did. Without the flag nothing changes.
