### Fixed

- **Pages served by the reader open again on the Xteink readers** (#467). The setup with a phone stayed at "Opening the reader's own network…" and never showed the page's address, and a phone that joined got no page; the transfer page and the flash cards' phone page were hit the same way. The firmware opened the listener but never told the plugin that it was open, so no address was shown and every request was refused. It now says so (or says it failed), as the emulator does.
