### Changed

- **The firmware and plugin-job driver is made on demand** (#228, #248, #2654). The request table and install machine are made at the first firmware or plugin command and let go once nothing runs, asks or waits; what plugins may still read (the outcomes op 13 answers, a check's result) rests with the device's firmware host. A device that installs plugins no longer pays about 2 KiB for them at idle. A device that can boot two firmwares keeps its driver made.
