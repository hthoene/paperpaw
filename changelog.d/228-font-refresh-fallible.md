### Fixed

- **Choosing fonts while running no longer aborts on a short heap** (#228). After a plugin brings fonts and when plugins change, the choices of reading font, status font, key map preset, page frame and voice are offered, and the fonts chosen are opened, with every allocation checked; where there is no room the choices and fonts in use stay and a warning says so (the next refresh tries again). A native plugin's folder written with a trailing `/` is the same folder, an empty one is none, and a font file the device cannot read proposes no font for its role.
