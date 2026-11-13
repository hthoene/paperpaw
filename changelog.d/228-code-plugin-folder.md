### Fixed

- **Sleep can show a preset picture from a code plugin's folder again** (#228, #243). Root 1 now also shows the install folder `plugins/<id>/` of a running code plugin, read only and behind `uses.storage`, as it does for a running data plugin (`files.md` 1.1). This covers `core.files` `stat` and `list`, and `image_prepare`. The plugin's code (`code.ppn`, also under an 8.3 name) and the host's `.install-marks` are never shown, and every change there is `ERR_DENIED`. Beta 3's preset pictures such as `plugins/<id>/owl.png` show again instead of the fallback.
