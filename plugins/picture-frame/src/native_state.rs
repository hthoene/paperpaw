//! What the plugin keeps between starts (`state.txt` in its data folder, a
//! `key=number` line each): where the sleep screen's photos go on from, where
//! the next window of the server's photos starts and when the server was last
//! fetched from (Unix seconds, 0: never).
use alloc::string::String;

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    files::{Volume, read_all_on, replace_on},
    text::{concat, decimal_u64},
};

/// The file.
pub const FILE: &str = "state.txt";

/// The kept numbers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Kept {
    /// The photo the next sleep shows (each-sleep mode), by position.
    pub cursor: usize,
    /// Where the next window of the server's photos starts.
    pub window: usize,
    /// When a fetch last ended well, in Unix seconds.
    pub synced: i64,
}

impl Kept {
    /// What `volume` holds; nothing kept where there is no file or it is not
    /// ours.
    #[must_use]
    pub fn read(volume: &dyn Volume) -> Self {
        let Ok(Some(bytes)) = read_all_on(volume, FILE, 512) else {
            return Self::default();
        };
        Self::parse(&String::from_utf8_lossy(&bytes))
    }

    /// The numbers a text holds.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        let mut kept = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            match key {
                "cursor" => kept.cursor = value.parse().unwrap_or(0),
                "window" => kept.window = value.parse().unwrap_or(0),
                "synced" => kept.synced = value.parse().unwrap_or(0),
                _ => {}
            }
        }
        kept
    }

    /// Keeps them.
    ///
    /// # Errors
    /// What writing says; [`Error::NoMemory`].
    pub fn write(&self, volume: &dyn Volume) -> Result<(), Error> {
        let (mut a, mut b, mut c) = ([0u8; 20], [0u8; 20], [0u8; 20]);
        let text = concat(&[
            "cursor=",
            decimal_u64(self.cursor as u64, &mut a),
            "\nwindow=",
            decimal_u64(self.window as u64, &mut b),
            "\nsynced=",
            decimal_u64(self.synced.max(0).unsigned_abs(), &mut c),
            "\n",
        ])?;
        replace_on(volume, FILE, text.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "test assertions")]
    use super::*;
    use crate::test_volume::Mem;

    #[test]
    fn what_is_kept_is_read_back_and_a_missing_or_foreign_file_keeps_nothing() {
        let volume = Mem::default();
        assert_eq!(Kept::read(&volume), Kept::default());
        let kept = Kept {
            cursor: 7,
            window: 40,
            synced: 1_700_000_000,
        };
        kept.write(&volume).unwrap();
        assert_eq!(Kept::read(&volume), kept);
        volume.write(FILE, b"cursor=x\nnothing\nwindow=3").unwrap();
        assert_eq!(
            Kept::read(&volume),
            Kept {
                cursor: 0,
                window: 3,
                synced: 0
            }
        );
    }
}
