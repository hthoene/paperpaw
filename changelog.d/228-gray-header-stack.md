### Fixed

- **Decoding a picture on a nearly full heap no longer aborts** (#228). The decoded-picture
  writer of `paperpaw-image` writes its 12-byte header from the stack, and a row longer than
  the room it took at the start is written as it is instead of growing that room.
