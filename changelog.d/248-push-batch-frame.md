### Changed

- **More stack room on the X4 Pro** (#248). A plugin's batch of commands was checked on a frame of 2,016 bytes, 32 short of the S3's limit; the rarely used parts (Wi-Fi reservations, firmware and plugin installs) now have frames of their own, so the batch takes 224 bytes and the largest S3 frame is 1,824. Nothing a plugin sees changes.
