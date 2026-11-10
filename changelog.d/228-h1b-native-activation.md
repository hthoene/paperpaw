### Changed

- **Desktop native plugins are admitted by the kernel** (#228). The desktop loader hands every interface-7 library to `Kernel` through `CodeLoader::load_native` and `NativeAdmission::admit`, once per plugin before its first frame; the old flat-plugin path for interface-7 libraries is gone. A refused declaration lists the plugin as failed and nothing of it is mapped. A library that declares or registers anything is admitted (an events-only plugin loads); a library with nothing at all is invalid. All plugins admitted by one loader share one dispatch budget.
