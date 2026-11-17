### Changed

- **Waking from a light sleep no longer walks the library again** (#305). After every wake the library was walked anew, in case books were copied onto the card while the device slept. A card that was taken out loses its power and is noticed as a swapped card; one that still answers is as the sleep left it, so the X4 now walks the library again only after a start or a swapped card. A walk that was under way goes on where it was.
