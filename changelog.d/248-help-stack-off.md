### Changed

- **Hints switched off no longer take 4 KiB as screens change** (#248). The hints read the screen list on every change, with a 4 KiB reply buffer, even when they were switched off; now they read it only while hints are on or still open. What shows is unchanged.
