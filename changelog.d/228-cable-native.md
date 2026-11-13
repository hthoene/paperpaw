### Added

- **The cable's native server** (#228, #900). A native plugin serves the USB cable with `net_serve` on port 0: the computer's lines reach it through the kernel's cable link, bodies land in its `.received/` folder, answers go back as lines. The queue is bounded, every cable allocation is fallible, an answer waits for room, and a server on the cable needs a device place when served again on a network port. The transfer plugin's cable tests (`plugins/transfer/tests/cable_native.rs`) now run on a hosted X4-like kernel instead of being ignored.

### Fixed

- **A received upload is on the card before the plugin hears of it** (#228). The cable's last body part syncs the card first; on FAT an appended file's entry was not written yet, so a book sent over the cable could not be imported (`ERR_UNSUPPORTED`), and a power cut could lose a body already answered for. A card that cannot sync refuses the upload with 500.
- **A question Back closed is answered before anything else** (#228). Answers that wait when a service pass starts reach their askers before requests and events are served, so a computer's next request no longer finds a cancelled question still open.
