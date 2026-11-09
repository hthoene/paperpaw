### Added

- CI scans the X4 Pro's and the X4 Classic's firmware (`ppn5`) with `paperpaw-sign frames-s3` against `shells/xteink/frame-budget-s3.txt` and `frame-budget-s3-classic.txt`: every host entry a plugin can call and what it reaches stays under the 2 KiB stack band, and every call the scan cannot follow (ROM routines, calls through a vtable or a function pointer, jump tables) is listed one by one with its reason. The S3 build for that step uses legacy symbol mangling; only the names differ from the release's build.
- `frames-s3` budget lines name a function by its path or its raw symbol; the scan names a function by its symbol when another function of the image has the same path, and a line whose path matches more than one function reached is an error.
