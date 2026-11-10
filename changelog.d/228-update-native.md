### Changed

- **Update on interface 7: the page for an update file on the card** (#228). The update plugin has its
  native entry: the device checks each update file in the card's `updates` folder, and the page offers
  the newest one that fits with its version, who signed it (PaperPaw, or your own key shown by its
  fingerprint, which installs only with that key), and whether it is newer, older or the same (an
  older or the same version installs only when you choose it, told so); another firmware's image is
  offered as such, never blind. It sends the install to the device (which checks the file again and
  always asks you first), shows how far it is, restarts when the update is in place, says after the
  restart how it went and removes the file. A newer update on the card is offered once at start.
  The plugins an update leaves out and the dual-boot notice are not native yet; the plugin is not
  offered as a file until they are.
