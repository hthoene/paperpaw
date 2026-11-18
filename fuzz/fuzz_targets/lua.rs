//! Any text as one of KOReader's settings files, read and, where it
//! reads, written and read back as the same value.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_plugin_koreader::native_model as native;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    if let Ok(Some(value)) = native::parse(&text)
        && let Ok(written) = native::write(&value)
    {
        assert_eq!(native::parse(&written).ok().flatten(), Some(value));
    }
});
