### Changed

- **Update is an interface-7 plugin only** (#228). The old form of the update page is gone: the app links
  the native update plugin and no other, and the programs that do not run native plugins have no
  *Updates* row until they do. What the old page did that the native one does not yet, until the device
  gives the plugin what it needs: the dual-boot notice with its two buttons, plugins built in now that
  come back as files after an update, the fetch of plugins that went on after a restart, the page opening
  at once for a check run from an action, and another firmware's image at the top of the card. The files of the old fetch queue are no longer read.
