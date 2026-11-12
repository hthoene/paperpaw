### Changed

- **The screen queue makes room as it is needed** (#248). A screen callback reserves room for one batch per callback running, not for all four at once, and the room goes back on standby or memory pressure when nothing waits. About 1 KiB less peak heap on the X4 reference set; a refused reservation reports no memory and holds nothing.
