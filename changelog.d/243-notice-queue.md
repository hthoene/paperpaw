### Changed

- **Notices wait their turn** (#243). A notice shows one at a time; those that come meanwhile (or while a screen guards the device) wait in order, at most 4 (2 on the X4), the oldest dropped. A background work run may now send `command.notify` with the `announce` bit (device messages and rules the user set up); without it the command is still refused for a run.
