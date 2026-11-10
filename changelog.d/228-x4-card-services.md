### Added

- **The X4 firmware wires `core.work`, `core.document`, `core.library` and `core.files`** (#228, #243). The loader wires them with the network (`core.work` over the bridge's clock and network) and over the card (the books a plugin reads, the catalog, the card's files), and offers exactly those four to the plugins it admits, with the features `uses.storage` and `uses.library`. Nothing runs yet: interface-7 plugins stay held back by the stack gate. `core.secrets` follows with the device key's sealer (#411).
