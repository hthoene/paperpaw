### Changed

- **Update on interface 7: searching online and downloading** (#228). The update page's *Search online*
  button asks the update server (the address in the developers' settings, betas as you chose) and
  offers a newer PaperPaw with its size; *Download* saves it into the card's `updates` folder with its
  progress, checks its size and hands it to the page that installs it. The code plugins you have
  installed and the server has builds of for the update are fetched after it, and the device asks you
  before each is put in place. The weekly look for updates (when you turned it on) and the check a rule
  asks for run as declared work, at a cheap moment: when Wi-Fi is on anyway or the device charges.
  Not native yet: the plugins an update leaves out, an update that waits for plugins, the dual-boot
  notice and the badge on the update entry.
