### Added

- **Mail on interface 7: the connection to the servers and the account** (#228). The plugin connects to IMAP and SMTP servers over the host's sockets (TLS from the start or STARTTLS, a pinned key, the trust question's data), reads the account from the settings and the password from the secrets for each connection, and wipes what carried it. The screens that start a connection follow.
