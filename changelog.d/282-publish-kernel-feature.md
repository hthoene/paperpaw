### Changed

- **`check-publish` refuses a plugin with code that turns on the SDK's `kernel` feature** (#282). Directly in its `Cargo.toml` or through a crate it depends on: the message names the path, such as "plugin clock turns on paperpaw-sdk/kernel via fonts-default". The first-party plugins that need it are exempt by an explicit list (`tools/publish/src/kernel.rs`). Plugin interface 7 does not change.
