### Fixed

- **A network call without room no longer ends the firmware on devices with the cable** (#228). The first network call of a plugin makes the network service. On a profile with the cable server this took a second piece of memory that could not be refused; when there was none, the firmware stopped instead of answering "no memory". The cable's link is part of the service's own block now, and a call without room answers `ERR_NO_MEMORY` as on every other profile.
