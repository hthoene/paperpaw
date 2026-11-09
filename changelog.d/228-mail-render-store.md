### Added

- **Mail on interface 7, fourth slice** (#228). The reader of a message (headers, MIME parts, quoted-printable, base64, character sets, HTML, attachments) and what Mail keeps (the list of a folder, its counts, the folders, the messages read) are rewritten so that they give the same text and the same files as before, hold only a little memory at a time, and never abort when memory runs out. They are not yet used by the plugin that runs; the connection and the screens follow.

### Fixed

- A mail's HTML text that is cut at the length limit now ends as a plain prefix of the whole text, as the text of other mails does.
