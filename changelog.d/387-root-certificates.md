### Changed

- Secure connections (X4, X4 Classic): the firmware has about twenty root certificates built in (PaperPaw's own services, GitHub, the services of the plugins and the largest hosts) instead of Mozilla's whole list, which makes the firmware about 45 KB smaller (#387). The full list is the new store plugin *All certificates* (off at first, signed by PaperPaw, read from the card only when a server's root is not built in). A server under another root fails with "Certificate not known. Install 'All certificates' from the store."
