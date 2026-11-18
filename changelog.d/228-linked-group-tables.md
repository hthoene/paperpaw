### Changed

- **The built-in plugins keep their setting groups as a table in the program too** (#228, #248). A linked plugin's build script makes a table of its setting groups next to its settings table, and the kernel keeps a group that equals an entry as a reference to it instead of a copy on the heap (`Kernel::register_linked` takes the plugin's tables, `paperpaw_app::fixed_tables`).
