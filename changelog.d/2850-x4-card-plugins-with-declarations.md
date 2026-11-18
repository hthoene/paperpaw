### Added

- **Card plugins that declare screens, settings and actions load on the X4 family** (#2850). A plugin file of interface 7 is hosted for the kernel to admit with everything it declares, all or none, as on the desktop; before, the X4 refused every plugin that declared anything. Each entry into a card plugin's code names it to safe mode and checks the stack's end afterwards; a refused entry (the band is not armed) runs nothing. The firmware now hands plugins its heap table, so plugins built with the SDK's default heap word are no longer refused.
- Plugin images are placed once per start: after the start's last one nothing is placed or taken back, so a hosted image stays where it is until a restart.
