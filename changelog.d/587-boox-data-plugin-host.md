### Changed

- **Boox emulator profile installs data plugins through the kernel's data-only plugin host** (#244, #228, #587). A device that has a catalogue name but loads no plugin code now gets the firmware's job queue with a stand-in for the firmware (`NoFirmware`) and `Kernel::set_data_plugin_host`, so a native store's install, check and removal of a data plugin are taken there; a plugin with a code file ends with reason 19 (`ERR_UNSUPPORTED`). The X4 profiles are unchanged.
