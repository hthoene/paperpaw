### Changed

- **Store on interface 7: who installed a plugin comes from the device** (#228, #244). The store's installs say they come from its catalog, and it reads from the registry which versions it put in place and as which release (betas included), so it offers updates only for those, as in Beta 3. It no longer keeps a record of its own; plugins it installed before keep their updates.
- **SDK: `Evidence::from_catalog`** (#244) marks a plugin install as a store's (`install_plugin` flags bit 2).
