### Changed

- **Wi-Fi: the radio's events on interface 7** (#228). The Wi-Fi plugin now brings the radio on when a plugin wants the network and joins the network joined before, looks again after a scan, hears a join that failed (a wrong password stops being pending, the next network is tried or the failure told), switches the radio off after its idle time (as a timer through the device's work service) and at once when home or a book shows, and says so when no network is coming. The phone setup and the typed password's keeping once online follow with their parts.
