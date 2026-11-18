### Changed

- **A native plugin's event and draw registrations are admitted with its declarations** (#228). An event registration needs a tag the host encodes and each tag only once; a draw needs the token of a screen or widget the plugin declared and each token only once, also across batches. A registration that does not fit refuses the whole plugin with nothing published, never on a full heap, and teardown drops the registrations before the owner stops. Events are not delivered to native plugins yet.
