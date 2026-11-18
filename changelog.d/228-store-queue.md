### Changed

- **The plugin store's queue and requests** (#228). The queue of plugins to fetch (*Update all*, plugins other plugins ask for, the first-run offer) and the requests of other plugins are kept on the card as before, now by the new interface-7 code: a queue or request left by Beta 3 is read as it is, and a refused allocation or a bad file ends in an error instead of a crash.
