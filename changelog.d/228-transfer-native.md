### Changed

- **Wi-Fi transfer on interface 7: the page, plugins, typing on a phone and the offers** (#228). The transfer plugin has
  its native entry: while its screen is open the device serves the web page to the local network, shows the address
  as text and as a QR code with a key drawn anew each time, and answers the page: the files list, a
  download, an upload to a free name when the card has room, a new folder and a delete. A few wrong keys
  lock the page for a minute. Closing the screen stops it. Plugins sent from the computer are put on the device file by
  file, each checked against its SHA-256, and installed once the device has asked you; the page for a book sent outside
  the library folder knows the formats the device reads. The phone's keyboard (*Phone* in the keyboard) serves a small page
  for typing and sends the text back. In the offline mode the screen offers to go online first, and *Without a router* opens
  the device's own network with a code to join it, closed after ten minutes without a request. Moving and renaming
  files and the USB cable wait for what they need from the library and the host; the plugin is not offered as a file
  until they are native.
