//! Owned observations copied during one draw callback. No borrowed host value survives.
use super::{abi::*, settings, snapshot};
use core::ffi::c_void;

pub(super) const SETTING_KEY: &[u8] = b"abi7-tick-draw.enabled";
const SETTING_CAPACITY: usize = 64;
const TEXT_CAPACITY: usize = SETTING_CAPACITY - settings::SETTINGS_HEADER_BYTES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Setting {
    Absent,
    Bool(bool),
    Int(i64),
    Text {
        bytes: [u8; TEXT_CAPACITY],
        len: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Observations {
    pub snapshot: snapshot::Snapshot,
    pub setting: Setting,
}

/// Table/context stay live; host callbacks synchronously initialize their outputs.
pub(super) unsafe fn read(host: &PpHost, ctx: *mut c_void) -> Result<Observations, i32> {
    // Request the current complete version-1 prefix. Extensions that cannot fit
    // must be refused by the host; no header offsets are reconstructed here.
    let mut bytes = [0u8; snapshot::SNAPSHOT_BYTES];
    // SAFETY: exclusive initialized output is disjoint from table and context.
    let status = unsafe {
        (host.env_snapshot)(
            ctx,
            PpBuffer {
                ptr: bytes.as_mut_ptr(),
                cap: bytes.len(),
            },
        )
    };
    if status != OK {
        return Err(if status < 0 { status } else { ERR_INVALID });
    }
    let snapshot = snapshot::snapshot_decode(&bytes)?;
    let mut bytes = [0u8; SETTING_CAPACITY];
    // SAFETY: static initialized key and exclusive output do not overlap host data.
    let required = unsafe {
        (host.settings_read)(
            ctx,
            PpBytes {
                ptr: SETTING_KEY.as_ptr(),
                len: SETTING_KEY.len(),
            },
            PpBuffer {
                ptr: bytes.as_mut_ptr(),
                cap: bytes.len(),
            },
        )
    };
    if required < 0 {
        return Err(required);
    }
    let required = required as usize;
    if required > bytes.len() {
        return Err(ERR_NO_SPACE);
    }
    let setting = match settings::settings_decode(&bytes[..required])? {
        settings::SettingValueRef::Absent => Setting::Absent,
        settings::SettingValueRef::Bool(value) => Setting::Bool(value),
        settings::SettingValueRef::Int(value) => Setting::Int(value),
        settings::SettingValueRef::Text(value) => {
            let mut bytes = [0; TEXT_CAPACITY];
            bytes[..value.len()].copy_from_slice(value.as_bytes());
            Setting::Text {
                bytes,
                len: value.len(),
            }
        }
    };
    Ok(Observations { snapshot, setting })
}
