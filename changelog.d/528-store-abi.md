### Fixed

- **The store lists every code plugin made for this PaperPaw** (#528). The
  store asked the plugin library for its list and plugin pages without
  naming the plugin interface, so the library answered with builds for an
  older PaperPaw and only about half of the code plugins were listed. It now
  names the interface (`abi=7.<minor>`) in every request.
