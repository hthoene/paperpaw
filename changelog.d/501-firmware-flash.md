### Changed

- **Smaller firmware images** (#501). The X4 Pro's, the X4 Classic's and the Boox Picco's firmware no longer carries an unwinding table that nothing read (about 119 KB each); the panic messages of every device name their source files by shorter paths, and the German language pack the firmware carries is compressed better (2,219 bytes). Measured on release-like local builds: X4 11,760 bytes smaller, X4 Classic 131,504 bytes smaller.
