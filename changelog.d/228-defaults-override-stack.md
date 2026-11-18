### Fixed

- **Setting defaults keep an override stack** (#228). When a plugin that set
  another plugin's setting default is removed, the default set by the plugin
  below it comes back, not the original one; the original returns when the
  last one goes.
