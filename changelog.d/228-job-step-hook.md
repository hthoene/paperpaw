### Added

- Shells can now tell when plugin code runs as a step of a host background job (the library's cover job, a book's details job) through the new `native_host::job_step::set(begin, end)` hook. `begin` and `end` run around each such call into a format plugin, and `end` also runs when the call fails. The X4 uses them so that a crash inside a job step counts against the job's book, not as a failed start (#228).
