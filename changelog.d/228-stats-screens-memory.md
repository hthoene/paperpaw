### Changed

- **Statistics screens on interface 7: bounded memory** (#228). The books screen lists the 200 books read last (most recent first) and cuts long titles and author lines to 80 bytes; the calendar keeps only each book's id and a cut title. Both declare the `core.ui` service and the host calls it needs.
