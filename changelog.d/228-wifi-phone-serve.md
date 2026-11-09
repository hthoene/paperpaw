### Changed

- **Wi-Fi: the setup with a phone serves its page on interface 7** (#228). The device opens its own
  network, serves the setup page on port 80 and answers the phone through the host's server; a device
  that serves another page already says so, and closing the screen stops serving and leaves the own
  network. The join code and the page's address show as QR codes under the rows; a short memory no
  longer stops the device for them.
