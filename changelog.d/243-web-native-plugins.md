### Changed

- **The browser emulator hosts the first-party native plugins** (#243, Decision 172). The web build now runs the linked interface-7 plugins on wasm32 as the other shells do: their HTTP requests go through the page's own network (the recorded answers and the page's fetches, as for interface 6), and their passwords are sealed with a key made for the page, kept in memory (nothing is saved across a reload, as before). `core.sockets` is not offered in the browser.
