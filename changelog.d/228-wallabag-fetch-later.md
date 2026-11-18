### Added

- **Wallabag fetches at a good moment when a rule starts it** (#228). *Fetch articles* fetches at once when you run it; when a rule (or another plugin) runs it, the fetch waits until the device is awake and the network is there, within an hour, as in Beta 3.
