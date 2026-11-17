### Changed

- **The X4's boot reads its flash at 80 MHz** (#304). The X4's flash chip (Puya PY25Q128HA, rated 104 MHz for DIO reads) is now clocked at 80 MHz by the boot loader instead of 40 MHz, which shortens the start. The X4 Pro, X4 Classic and X3 keep 40 MHz until their flash chips are known.
