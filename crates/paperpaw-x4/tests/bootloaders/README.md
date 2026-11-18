# Bootloaders scanned for a false dual-boot detection (#224)

`../bootloaders.rs` scans each of these as the firmware does (`dual_boot::scan_marker`, the first 32 KB
of the flash) and asserts that none is taken for a dual-boot setup. Files here are builds of ESP-IDF
(Apache-2.0); the others are listed by hash only (vendor or unknown licence). Checked 2026-11-04.

| Bootloader | Chip | Where from | SHA-256 | Detected |
|---|---|---|---|---|
| PaperPaw 0.6.0-beta.2, X4 and X3 | C3 | release `paperpaw-x4-0.6.0-beta.2.bin` and `…-x3-…`, first 0x8000 bytes (21,072 B used) | ff342356…3a7b | no |
| PaperPaw 0.6.0-beta.2, X4 Pro and X4 Classic | S3 | release `paperpaw-x4-pro-…bin` and `…-x4-classic-…bin` (21,072 B used) | 07d68375…290e | no |
| CrossPoint X4 | C3 | crosspoint-reader/crosspoint-tools `public/firmware/bootloader.bin` | 6e6b0d38…9fb6 | no |
| CrossPoint's X4 Pro one (same bytes as their lilygo, papermono, sticky) | S3 | crosspoint-tools `x4pro-bootloader.bin` | 09c6a477…19fb | no |
| *hash only* CrossPoint metalio | S3 | crosspoint-tools `metalio-bootloader.bin` | 15f88ae1…a3ca | no |
| *hash only* lowio's hosted "stock" X4 bootloader | C3 | lowio.xyz/tools/dualboot-x4/bins/bootloader-stock.bin (20,096 B) | 0541aa80…c36b | no |
| *hash only* test X4's bootloader as found on 2026-11-03 | C3 | read-only dump by device-tester (`b0-boot.bin`, 20,560 B used) | 6f63909c…754d (used part) | no |
| *hash only* owner's X4 as found 2026-11-04 | C3 | read-only dump (21,072 B used) = the PaperPaw C3 bootloader above | ff342356… | no |
| **lowio dualbooter's** | C3 | lowio.xyz/tools/dualboot-x4/bins/bootloader.bin (21,712 B) | 9855f7b2…3dfd | **yes**, `x4boot` at 0xe0 (its tag with the bytes around it is tested in `dual_boot.rs`) |

The stock Xteink factory bootloader is not published separately; lowio's "stock" file and the test X4's
dump are the closest known (the test X4's was not CrossPoint's nor PaperPaw's). If one is ever found to
be detected, `dual-boot = off` in `paperpaw.ini` is the way out, and the match is to be tightened.

A release check: `PAPERPAW_BOOTLOADER_DIR=<folder with the release's full images> cargo test -p paperpaw-x4 --test bootloaders`.
