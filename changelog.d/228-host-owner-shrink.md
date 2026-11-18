### Changed

- **Less host memory for each hosted plugin** (#248). A plugin's host context no longer keeps its own pointer to the shared host table or a second view of its resources (its callback's blobs expire through the context's own), and the identity its dispatch and its parking recipe keep holds the device pool, not an owner's view of it. Nothing changes in behaviour.
