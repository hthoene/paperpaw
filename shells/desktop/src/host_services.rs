//! The desktop emulator's part of the interface-7 host table (ADR 0034, M1):
//! the process's heap and clock behind [`paperpaw_native::host_glue`], and the
//! table with its context, built once for the process. Shared with the other
//! shells (`paperpaw_shell_glue::services`).

pub use paperpaw_shell_glue::services::{
    StdHost as DesktopHost, StdServices as DesktopServices, std_host as desktop_host,
};
