### Changed

- **Updates can be downloaded packed: a third smaller** (#304). Each release now also offers `-update-packed.bin`: the same firmware deflated (about 3.8 MB instead of 6.0 MB for the X4). The device inflates it while it writes it into the other slot; the signature (Ed25519) covers the packed file and so fixes the image it unpacks to, and a damaged file fails as any other does. Beta 3 and older do not read the format and refuse the file; the plain `-update.bin` stays for them. `paperpaw-sign pack-update` makes the file, `paperpaw-sign show` reads both.
