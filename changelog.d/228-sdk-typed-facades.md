### Changed

- **Plugins link only the records they use** (#228). The SDK's library, device,
  network and lookup calls now decode and encode the record types they name
  instead of going through the generic record enum, which pulled every record
  type of the wire into every plugin. A plugin file that reads the library
  (End of book) is 137 KB smaller (235,828 → 98,624 bytes). Nothing a plugin
  calls changes. `tools/plugin-symbols.sh` and a source check keep it so.
