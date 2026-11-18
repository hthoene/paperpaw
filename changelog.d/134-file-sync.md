### Added

- File sync: keeps folders of the card in sync with your own WebDAV server (Nextcloud, ownCloud, a NAS and the like), in both directions or from the server only (a store plugin, #134). Three pairs of folders, each with its own server, user and password (kept among the passwords and keys). A file changed on one side goes to the other; changed on both, both are kept (the card's copy as `name (conflict device date).ext`); deleted on one side and unchanged on the other, it is deleted on the other too (the card's copy goes to a trash of the last 100), else it stays; many deletions at once are held back. Files are written whole or not at all and a cut or a lost network is picked up where it stopped. Syncs on request (Tools, or the widget *Sync now*), with the charger or where Wi-Fi is up anyway; shows progress and a summary, and has the widgets *Last sync* and *Pending changes*.

### Changed

- The WebDAV address, account and listing code that Picture frame had is now shared in the UI kit (`paperpaw_ui_kit::dav`) with File sync.
