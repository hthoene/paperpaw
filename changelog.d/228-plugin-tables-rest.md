### Changed

- **Nine more linked plugins keep their settings, setting groups and ids in the program** (#248). About, files, mail, notes vault, plugin manager, setup, transfer, update and Wi-Fi get their tables made by their build scripts from the very declarations they submit, as menu does, so the kernel no longer copies each setting, group and id of them onto the heap at start. No behaviour change.
