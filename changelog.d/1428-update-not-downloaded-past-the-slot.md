### Changed

- **An update that does not fit the device's app slot is no longer downloaded** (Decision 179). On a device with the small CrossPoint layout (slots of 6.25 MB) the update page compares the offer's size with the slot before it starts the download and says so, instead of loading about 7 MB and refusing it at the install. The notice (English and German) says the update does not fit this device's storage layout and to install PaperPaw once with the flasher at paperpaw.org/flash; updates work on the device after that. The device's own refusal before the first write is unchanged.
