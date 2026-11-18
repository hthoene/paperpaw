### Changed

- **Wi-Fi on interface 7, third part** (#228). The networks joined before are kept among the
  plugin's secrets, and the rules of the device's own network for a phone (its password, the page's
  token and the code that joins it) are in place without the host and refuse cleanly when memory is
  short. The passwords drawn and kept are wiped after use.
