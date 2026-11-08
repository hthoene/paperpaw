### Added

- **Plugins can declare stages and subscribe to them** (#228). A native plugin's `stage.spec` and `registration.stage` records are admitted instead of refused as unsupported. A plugin names each stage once, subscribes a token to a stage once, and has at most eight subscriptions (a ninth is refused as full); a subscription to its own stage is refused. A subscription to a stage nobody has installed is kept and stays inactive. Stages and subscriptions go away with their plugin.
