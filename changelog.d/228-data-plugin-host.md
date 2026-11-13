### Fixed

- **Data plugins install on devices that load no plugin code** (#244, #228). On Boox/Android, desktop and web, an install of a data plugin (a font, a dictionary: `plugin.ini` and files, no code) was refused with `ERR_UNSUPPORTED`, because only the ESP chips had a plugin host. A shell can now set a data-only plugin host (`KernelBuilder::data_plugin_host`, `Kernel::set_data_plugin_host`) on the firmware queue: plugin jobs install, check and remove data plugins, and a plugin with a code file ends with reason 19 (`ERR_UNSUPPORTED`), as code for another kind of device does.
