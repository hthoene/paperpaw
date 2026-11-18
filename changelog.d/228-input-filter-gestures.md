### Added

- **A native input filter can raise inputs and make gestures** (#228). A filter that answers `raise` with an input or gesture its plugin declared (`input.spec`) makes the layers shown run what they bind to it, as if it came from the device; a name the plugin did not declare leaves the key as it was. A filter that replaces an input with a gesture (`input.gesture`: tap, long press or a swipe) gives the screen on top that gesture at a point on the display.
