### Fixed

- **Picture requests that fail always get an answer** (#228). A `command.image_prepare` the table refuses for good at apply (the submit already said yes) is told to the plugin as `event.image_ready` status 1; failures that wait for their telling give way oldest first, and the requests a plugin has held back count against its room, so a plugin cannot flood them. The service pump no longer runs a waiting batch inside a callback. Known limit: with more than eight failing requests across plugins at once, the oldest failure is dropped and its plugin is not told of that request.
