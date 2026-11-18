### Added

- The S3 firmware (X4 Pro and X4 Classic, `ppn5`) exports its host table as a named read-only static, `PAPERPAW_HOST_TABLE_ANCHOR` (about 150 bytes of flash), so `paperpaw-sign frames-s3` can read the table's function pointers from the ELF and use them as the roots of its scan. The table plugins are handed is unchanged.
