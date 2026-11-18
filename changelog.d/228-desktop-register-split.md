### Changed

- **The emulator's linked-plugin registration is split** (#228). `register` in `shells/desktop/src/linked.rs` was over clippy's 100-line limit with `native-linked`; the part that says which services are wired is its own function. No change of behaviour.
