### Changed

- **Native SDK: plugins link only the record codecs they use** (#1156, #228). The generated
  records codec has a typed form (`WireEncode`, `WireRecord`, `RawRecord`, the `_of` encoders,
  `RecordStream::feed_raw`) and every SDK facade encodes and decodes by record type. Battery's
  X4 plugin file shrinks from 239 KB to 90 KB; the wire format is unchanged. Interface draft 24
  (`native::DRAFT`) refuses plugins built for draft 23.
