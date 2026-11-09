### Changed

- **Update on interface 7: the page for an update file on the card** (#228). The update plugin has its
  native entry: it lists the update files in the card's `updates` folder, sends the install to the
  device (which checks the file and always asks you first, and shows the version and who signed it),
  shows how far it is, restarts when the update is in place, says after the restart how it went and
  removes the file. A file the device says is not newer installs only when you choose the older
  version. Searching online, downloading, the plugins' fetch, the weekly look and the states that
  need the file's version and signer wait for the device's file check; the plugin is not offered as a
  file until they are native.
