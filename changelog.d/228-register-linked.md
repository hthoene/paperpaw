### Added

- **`Kernel::register_linked`** (#228). A plugin linked into the program is
  checked (entry, metadata view, target profile, versions and requirements
  against the shell's card-plugin host, passed in), hosted and registered like a loaded one.
  An entry or metadata the kernel refuses, an id that is not a plugin id and
  an id already registered are refused before the plugin's factory runs.
