### Added

- **Mail on interface 7, third slice** (#228). The readers of IMAP responses and SMTP replies, and the builders of the login, address and data lines, run without panics and report out-of-memory instead of aborting. They give the same answers as before; a server's line is held to 8 KiB, the socket buffer to 16 KiB, a line to 256 tokens and a capability or flag list to 64 words, a fetch line that ends in a space no longer loses its items, and the login line with the password wipes itself from memory.
