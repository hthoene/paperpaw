### Fixed

- **X4 Classic: home screen, plugins and books work again** (#415). The stack-guard's self-check expected the chip's monitor registers to read back the full 32-bit addresses, but the ESP32-S3 keeps only 26 address bits and 16-byte steps. The guard never armed on the Classic, so no plugin ran and Home stayed empty. The check now expects what the chip holds, and the log names the reason when the guard does not arm.
