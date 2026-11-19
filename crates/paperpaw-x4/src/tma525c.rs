//! The Boox Picco's touch screen: a Parade (Cypress) TMA525C on I²C, which
//! speaks the PIP protocol. Every read is a report: two bytes of its length
//! (little endian, the two included), then the report read again whole; a
//! length below 2 says there is none. The controller pulls its interrupt
//! line low while a report waits.
//!
//! A touch report (id 0x01): the number of records in the low five bits of
//! byte 5, then records of 10 bytes from byte 7: in a record's byte 1 the
//! finger's id (low five bits) and what it did (bits 5 and 6, 3 for lifted),
//! then x and y (little endian) in the controller's frame, 480 × 800 with
//! both axes the other way round from the upright screen.
//!
//! The protocol as the FreeInk SDK's `InputManager` has it (MIT licence,
//! Copyright (c) 2026 FreeInk), read there from the Picco's stock firmware
//! (build 2912). Not tried on a device by PaperPaw.

use embedded_hal::i2c::I2c;

/// The address it answers at.
pub const ADDRESS: u8 = 0x24;
/// The controller's frame.
pub const WIDTH: u16 = 480;
pub const HEIGHT: u16 = 800;
/// The longest report read: a touch report of [`MAX_POINTS`] fingers.
pub const REPORT: usize = 64;
/// Fingers read from one report, at most.
pub const MAX_POINTS: usize = 5;

/// A touch report's id.
const TOUCH: u8 = 0x01;
/// The mode byte of the descriptor: the application runs, or its boot
/// loader does.
const MODE_APP: u8 = 0xF7;
const MODE_BOOT: u8 = 0xFF;
const HEADER: usize = 7;
const RECORD: usize = 10;

/// A finger on the screen, in the controller's coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Point {
    pub id: u8,
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    I2c,
    /// A report longer than [`REPORT`], or cut short.
    Report,
}

/// One report into `out`: its length, 0 if none waits.
pub fn report<I: I2c>(i2c: &mut I, out: &mut [u8; REPORT]) -> Result<usize, Error> {
    let mut length = [0u8; 2];
    i2c.read(ADDRESS, &mut length).map_err(|_| Error::I2c)?;
    let length = usize::from(u16::from_le_bytes(length));
    if length < 2 {
        return Ok(0);
    }
    if length > REPORT {
        return Err(Error::Report);
    }
    i2c.read(ADDRESS, &mut out[..length])
        .map_err(|_| Error::I2c)?;
    Ok(length)
}

/// Asks for the descriptor (register 1): the reply's third byte is the
/// mode. The reply is read with [`report`] once the interrupt line is low.
pub fn ask_mode<I: I2c>(i2c: &mut I) -> Result<(), Error> {
    i2c.write(ADDRESS, &[0x01, 0x00]).map_err(|_| Error::I2c)
}

/// What the descriptor's reply says is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// The application: touches are reported.
    App,
    /// The boot loader: [`launch`] starts the application.
    Boot,
    Unknown,
}

/// The mode a descriptor's reply names.
pub fn mode(reply: &[u8]) -> Mode {
    match reply.get(2) {
        Some(&MODE_APP) => Mode::App,
        Some(&MODE_BOOT) => Mode::Boot,
        _ => Mode::Unknown,
    }
}

/// Tells the boot loader to start the application (its command 0x3B in a
/// packet with its CRC).
pub fn launch<I: I2c>(i2c: &mut I) -> Result<(), Error> {
    let mut packet = [
        0x04, 0x00, 0x0B, 0x00, 0x40, 0x00, 0x01, 0x3B, 0x00, 0x00, 0x00, 0x00, 0x17,
    ];
    let crc = crc(&packet[6..10]);
    packet[10..12].copy_from_slice(&crc.to_le_bytes());
    i2c.write(ADDRESS, &packet).map_err(|_| Error::I2c)
}

/// Puts the controller to sleep, before the device's deep sleep.
pub fn sleep<I: I2c>(i2c: &mut I) -> Result<(), Error> {
    i2c.write(ADDRESS, &[0x05, 0x00, 0x01, 0x08])
        .map_err(|_| Error::I2c)
}

/// CRC-16/CCITT-FALSE, as the boot loader's packets carry it.
fn crc(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in bytes {
        crc ^= u16::from(byte) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ 0x1021
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// The fingers down in the touch report `report` into `points`: how many
/// (0: all lifted). `None` if it is no touch report, or one cut short or
/// with a finger outside the frame: nothing to act on.
pub fn touches(report: &[u8], points: &mut [Point; MAX_POINTS]) -> Option<usize> {
    if report.len() < HEADER || report[2] != TOUCH {
        return None;
    }
    let count = usize::from(report[5] & 0x1F);
    let records = report.get(HEADER..HEADER + count * RECORD)?;
    let mut n = 0;
    for record in records.as_chunks::<RECORD>().0 {
        // Lifted: no longer down.
        if (record[1] >> 5) & 0x03 == 3 {
            continue;
        }
        let x = u16::from_le_bytes([record[2], record[3]]);
        let y = u16::from_le_bytes([record[4], record[5]]);
        if x > WIDTH || y > HEIGHT {
            return None;
        }
        if let Some(point) = points.get_mut(n) {
            *point = Point {
                id: record[1] & 0x1F,
                x,
                y,
            };
            n += 1;
        }
    }
    Some(n)
}

/// The upright screen's pixel under the controller's `(x, y)`: both axes
/// run the other way (the stock firmware's `480 - x`, `800 - y`).
pub fn upright(x: u16, y: u16) -> (u16, u16) {
    (
        (WIDTH - x.min(WIDTH)).min(WIDTH - 1),
        (HEIGHT - y.min(HEIGHT)).min(HEIGHT - 1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch_report(records: &[[u8; RECORD]]) -> alloc::vec::Vec<u8> {
        let mut report = alloc::vec![0, 0, TOUCH, 0, 0, records.len() as u8, 0];
        for record in records {
            report.extend_from_slice(record);
        }
        report
    }

    #[test]
    fn a_finger_is_read_and_a_lifted_one_is_not() {
        let mut points = [Point { id: 0, x: 0, y: 0 }; MAX_POINTS];
        let down = [0, 0x23, 0x10, 0x00, 0x20, 0x03, 0, 0, 0, 0];
        assert_eq!(touches(&touch_report(&[down]), &mut points), Some(1));
        assert_eq!(
            points[0],
            Point {
                id: 3,
                x: 16,
                y: 800
            }
        );
        let lifted = [0, 0x63, 0x10, 0x00, 0x20, 0x03, 0, 0, 0, 0];
        assert_eq!(touches(&touch_report(&[lifted]), &mut points), Some(0));
    }

    #[test]
    fn what_is_no_whole_touch_report_is_nothing() {
        let mut points = [Point { id: 0, x: 0, y: 0 }; MAX_POINTS];
        let mut report = touch_report(&[[0; RECORD]]);
        report.pop();
        assert_eq!(touches(&report, &mut points), None);
        let other = [0, 0, 0x1F, 0, 0, 0, 0];
        assert_eq!(touches(&other, &mut points), None);
        let outside = [0, 0, 0xFF, 0xFF, 0, 0, 0, 0, 0, 0];
        assert_eq!(touches(&touch_report(&[outside]), &mut points), None);
    }

    #[test]
    fn the_frame_lies_the_other_way_round() {
        assert_eq!(upright(480, 800), (0, 0));
        assert_eq!(upright(1, 1), (479, 799));
        assert_eq!(upright(0, 0), (479, 799));
    }

    #[test]
    fn the_modes_and_the_launch_packet() {
        assert_eq!(mode(&[0, 0, 0xF7]), Mode::App);
        assert_eq!(mode(&[0, 0, 0xFF]), Mode::Boot);
        assert_eq!(mode(&[0, 0]), Mode::Unknown);
        // CRC-16/CCITT-FALSE of "123456789".
        assert_eq!(crc(b"123456789"), 0x29B1);
    }
}
