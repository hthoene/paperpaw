### Fixed

- **Community plugin sources are read by a store packed as a plugin file** (#320, #214): such a store checks a source's signature with an Ed25519 of its own whose largest stack frame is about 1 KiB, in place of the kernel's, whose frame of about 4 KiB no plugin file may have. It refuses and takes exactly what the kernel's does (a forged or changed list, a signature that is not canonical, a key of a small order). The store file for the X4 Pro and X4 Classic took no source's signature before, and the one for the X4 was refused by the packer; the firmware's linked store is as before.
