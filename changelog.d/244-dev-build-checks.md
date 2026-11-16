### Fixed

- **`paperpaw-dev build` checks what it is given before it builds** (#244): a package name that is no plugin id is refused before any folder is named or removed by it; a `paperpaw-sign` of another PaperPaw release is refused with how to install the right one (`paperpaw-sign --version` is new); and `CARGO_ENCODED_RUSTFLAGS` no longer reaches the plugin's build.
