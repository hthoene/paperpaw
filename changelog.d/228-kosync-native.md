### Changed

- **Progress sync starts its move to interface 7** (#228). Its settings and the logic that decides what a fetched position means, writes the progress and reads the server's answer now exist natively beside the old code; the sync itself, the account screen and the overlay follow when the network host and secrets reach plugins.
