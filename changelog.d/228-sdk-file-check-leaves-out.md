### Added

- **`FileCheck::leaves_out`** (#228). A finished firmware check says whether the checked image leaves out plugins built into the running one (`dev_check.flags` bit 5), so the update view can skip `Device::leaves_out` when nothing is left out.
