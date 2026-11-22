### Fixed

- **A plugin can start its web server from an event callback again** (#469).
  Before, a plugin that started its server outside its screen's own
  callback (for example after Wi-Fi came up) was refused until it had used
  background work once; now it serves from its showing screen as
  documented, which the phone setup of Wi-Fi needs.
