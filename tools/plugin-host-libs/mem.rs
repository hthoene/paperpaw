//! The memory functions the compiler calls for copies and comparisons. A
//! shared library the emulator's interface-7 loader takes imports nothing, not
//! even these (docs/plugins.md, "code-host.so"), so the library brings its own
//! (x86-64, `rep` string instructions), and two unwinder stubs, and keeps them
//! hidden: only the two entry symbols are exported.

core::arch::global_asm!(include_str!("mem.s"));
