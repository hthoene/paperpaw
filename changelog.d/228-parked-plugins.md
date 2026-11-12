### Changed

- **Parked native plugins, kernel side** (#248, #2638). A native plugin's lease holds its live instance or, for a parked plugin, the recipe that makes it again at a safe point (`ERR_BUSY` and a start on the next pass elsewhere, a back-off after no memory, withdrawal at the pass boundary on a refused batch). The setting and stack-event pumps make a parked subscriber before telling it and keep its event where they cannot. Nothing is parked yet.
