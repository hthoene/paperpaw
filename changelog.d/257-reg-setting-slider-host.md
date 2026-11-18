### Added

- **Settings that ask for a slider say so in the registry** (#257). `reg_setting.flags` bit 11 is set for an int setting whose extra asks for a slider (the kernel's margins and sizes among them), so the native settings and menus show Beta 3's slider; switches need nothing new (bit 6 of `reg_menu_row`).
