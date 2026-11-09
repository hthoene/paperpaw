### Added

- **Mail on interface 7, fifth slice** (#228). The IMAP and SMTP conversations with a mail server (connect, secure with TLS or STARTTLS, log in, read the folders, the headers and a message, set a flag, send a message) are rewritten so that they say the same things to the server and leave the same files, never keep the password in a copy that is not wiped, close the connection on a server's answer that makes no sense, and tell the reader when memory runs out instead of stopping. They are not yet connected to the network by the plugin that runs; the connection and the screens follow.
