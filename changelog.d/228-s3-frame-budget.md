### Added

- CI scans the X4 Pro's firmware (`ppn5`) with `paperpaw-sign frames-s3` against `shells/xteink/frame-budget-s3.txt`: every host entry a plugin can call and what it reaches stays under the 2 KiB stack band, and every call the scan cannot follow (ROM routines, calls through a vtable or a function pointer, jump tables) is listed one by one with its reason. The S3 build for that step uses legacy symbol mangling; only the names differ from the release's build.
