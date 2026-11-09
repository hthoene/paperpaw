### Changed

- **The reading aids answer the reader's rules stage** (#228). On the native interface the plugin
  subscribes to `reader.filter` and answers with the rules of its three settings (bold word
  beginnings, replaced words, pictures kept, captioned or left out; the caption word of the device's language). A configuration the answer cannot hold is refused, not cut short. The reader applies the
  rules once its native screens ask the stage.
