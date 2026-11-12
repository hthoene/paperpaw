### Changed

- **A plugin install holds less while it runs** (#228, #248). The copy's walk and file list go before the stage check, and the 4 KiB buffer goes after the trust rules (nothing later reads through it), so neither is held while the user decides or through the swap. A firmware or plugin request reserves the room of one request exactly, not four.
