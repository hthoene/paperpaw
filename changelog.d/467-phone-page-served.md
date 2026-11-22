### Fixed

- **Wi-Fi setup with a phone serves its page again on the Xteink readers** (#467). The setup screen said "The page could not be served: There is no network." as soon as the device's own network opened: the Wi-Fi plugin asks for its page in answer to the radio's news that the network is up, and the firmware's network bridge still went by the step before, when there was none. The bridge now takes the radio's network before the plugins hear of it, so the page is served and its address shown.
