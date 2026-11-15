//! Fixed-size codec for `payloads/reading-goal.read-today-reply@1.def`.
//! Shared with stats: one record, no optional fields or variable-size values.
#![allow(
    dead_code,
    reason = "the producer and consumer use different halves of this shared codec"
)]

pub mod reading_goal_read_today_reply_v1 {
    // Batch count, local record tag, body length, empty optional-field bitmap.
    const HEADER: [u8; 4] = [1, 1, 13, 0];
    const BYTES: usize = 16;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct TodayReading {
        pub known: u32,
        pub day: i32,
        pub seconds: u32,
    }

    impl TodayReading {
        pub const fn new(known: u32, day: i32, seconds: u32) -> Self {
            Self {
                known,
                day,
                seconds,
            }
        }

        pub fn encode(&self, out: &mut [u8]) -> Result<usize, i32> {
            if out.len() < BYTES {
                return Err(-3);
            }
            out[..4].copy_from_slice(&HEADER);
            out[4..8].copy_from_slice(&self.known.to_le_bytes());
            out[8..12].copy_from_slice(&self.day.to_le_bytes());
            out[12..16].copy_from_slice(&self.seconds.to_le_bytes());
            Ok(BYTES)
        }

        pub fn decode(bytes: &[u8]) -> Result<Self, i32> {
            let bytes: &[u8; BYTES] = bytes.try_into().map_err(|_| -2)?;
            if bytes[..4] != HEADER {
                return Err(-2);
            }
            Ok(Self {
                known: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                day: i32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
                seconds: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            })
        }
    }
}
