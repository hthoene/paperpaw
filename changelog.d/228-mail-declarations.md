### Added

- **Mail on interface 7, first slice** (#228). Mail's three accounts and their settings (provider, address, password, servers, ports, security, pinned keys, how many mails to fetch, mark as read, signature) are declared to the host as interface 7 asks, with the password kept as a secret. Reading and sending mail stay on the old interface until the sockets, secrets and background-work services are there.
