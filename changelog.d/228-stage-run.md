### Added

- **Native plugins can run a stage they own** (#228). `stage.run` (slot 32) from a native plugin now reaches its subscribers: the owner asks one subscriber per cursor, in the stage's order, and gets one `stage.reply` back. A subscriber that fails, answers too much or answers with something that is no record batch costs only its own reply a status. Subscribers whose plugin has not started yet and the user's per-stage switch are not part of this yet.
