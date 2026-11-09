### Changed

- **Plugin interface 7, draft 31** (#228): one bless for the approved SDK pieces of the carrier: server events, pictures (`image_prepare`, `ImageReady`, `Canvas::draw_image`), book covers, the `core.document` client (opening books, readers for "Open with"), annotation anchors and edit transactions, book data folders, format blocks, deleting and importing books, firmware installs from plugins (`install_firmware`, `firmware_restart`) and plugin servers (`Net::serve`, `respond`), and `Storage::same_entry`. Plugins built for draft 30 must be rebuilt.
