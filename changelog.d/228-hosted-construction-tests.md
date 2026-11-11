### Fixed

- **Hosted construction tests of fonts-browser and update** (#244). They still expected the host to refuse the plugins' `install_done` / `firmware_changed` registrations; the host admits them, so the tests now assert that construction succeeds.
