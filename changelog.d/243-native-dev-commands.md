### Fixed

- Native plugins' power, Wi-Fi and firmware commands now reach the device: every callback's command stage is admitted with the plugin's features, the kernel's Wi-Fi request table and, where wired, the firmware table, and the kernel applies them in their place among the batch's other commands. Before, they were refused as unsupported, so the native Wi-Fi page could not open (#2384).
