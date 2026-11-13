### Changed

- **A plugin install holds less while it runs** (#228, #248). The copy's walk and file list go before the stage check, and the 4 KiB buffer goes after the trust rules (nothing later reads through it), so neither is held while the user decides or through the swap. The first firmware or plugin request no longer holds room for four requests: each reserves exactly its own.
