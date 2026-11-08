//! Any text as one of KOReader's settings files, read and, where it
//! reads, written and read back the same; the native reader reads what the
//! interface-6 one reads and writes the same text.

#![no_main]

use libfuzzer_sys::fuzz_target;
use paperpaw_plugin_koreader::{lua, native_model as native};

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let old = lua::parse(&text);
    if let Some(value) = &old {
        assert_eq!(lua::parse(&lua::write(value)).as_ref(), Some(value));
    }
    let new = native::parse(&text).ok().flatten();
    assert_eq!(new.is_some(), old.is_some());
    if let (Some(new), Some(old)) = (new, old) {
        let written = native::write(&new).unwrap();
        assert_eq!(written, lua::write(&old));
        assert_eq!(native::parse(&written).ok().flatten(), Some(new));
    }
});
