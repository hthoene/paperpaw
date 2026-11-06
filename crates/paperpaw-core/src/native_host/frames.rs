//! The `draw.frame` record (tag 192) that the host passes to `draw` (N2).
//!
//! `docs/abi/screens-input.md` section 6. The host encodes one frame per draw
//! call into a caller-owned buffer (a stack array: a framed record is at most
//! about 300 bytes) as a batch of exactly one record, like events. Nothing here
//! allocates, keeps the buffer or calls plugin code.

use super::registrations::{DrawFrame, Record, records_encode};

/// `draw.frame.purpose`: what the drawing is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Purpose {
    /// A screen of the plugin (`id` 0, no payload).
    Screen,
    /// A page layer.
    Layer,
    /// An inserted block.
    Insert,
    /// A status item.
    Status,
    /// A widget.
    Widget,
}

impl Purpose {
    /// The wire value. Other values are reserved: plugins draw nothing for them.
    #[must_use]
    pub const fn wire(self) -> u32 {
        match self {
            Self::Screen => 0,
            Self::Layer => 1,
            Self::Insert => 2,
            Self::Status => 3,
            Self::Widget => 4,
        }
    }
}

/// The frame of a whole screen: the rectangle is the screen's pane, the clip the
/// rows this call may change (the band on the X4, the whole pane elsewhere).
#[must_use]
pub const fn screen(
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    clip: (i32, i32),
) -> DrawFrame<'static> {
    DrawFrame {
        purpose: Purpose::Screen.wire(),
        x,
        y,
        width,
        height,
        clip_top: clip.0,
        clip_bottom: clip.1,
        id: 0,
        payload: &[],
    }
}

/// Encode `frame` as a one-record batch; returns the bytes written.
///
/// # Errors
/// `ERR_NO_SPACE` when the payload is over its 256-byte cap or `out` is too
/// small; the output is then unchanged.
pub fn encode(frame: &DrawFrame<'_>, out: &mut [u8]) -> Result<usize, i32> {
    records_encode(&[Record::DrawFrame(*frame)], out)
}

#[cfg(test)]
mod tests {
    use super::super::registrations::{DRAW_FRAME_PAYLOAD_MAX_BYTES, records_decode};
    use super::*;
    use paperpaw_native::host_glue::ERR_NO_SPACE;

    #[test]
    fn a_screen_frame_round_trips_with_every_field() {
        let mut out = [0; 320];
        let frame = DrawFrame {
            id: 7,
            payload: b"\x01\x02",
            ..screen(-3, 4, 480, 800, (16, 48))
        };
        let n = encode(&frame, &mut out).unwrap();
        let batch = records_decode(&out[..n]).unwrap();
        let mut records = batch.iter();
        let (Some(Record::DrawFrame(got)), None) = (records.next(), records.next()) else {
            panic!("not one frame");
        };
        let DrawFrame {
            purpose,
            x,
            y,
            width,
            height,
            clip_top,
            clip_bottom,
            id,
            payload,
        } = got;
        assert_eq!(
            (
                purpose,
                x,
                y,
                width,
                height,
                clip_top,
                clip_bottom,
                id,
                payload
            ),
            (0, -3, 4, 480, 800, 16, 48, 7, &b"\x01\x02"[..])
        );
    }

    #[test]
    fn the_largest_frame_fits_the_documented_stack_buffer() {
        let payload = [0xa5; DRAW_FRAME_PAYLOAD_MAX_BYTES];
        let frame = DrawFrame {
            purpose: Purpose::Widget.wire(),
            x: i32::MIN,
            y: i32::MIN,
            width: u32::MAX,
            height: u32::MAX,
            clip_top: i32::MIN,
            clip_bottom: i32::MAX,
            id: u32::MAX,
            payload: &payload,
        };
        let mut out = [0; 300];
        assert!(encode(&frame, &mut out).unwrap() <= 300);
    }

    #[test]
    fn an_over_cap_payload_or_small_buffer_is_no_space_and_writes_nothing() {
        let payload = [0; DRAW_FRAME_PAYLOAD_MAX_BYTES + 1];
        let frame = DrawFrame {
            payload: &payload,
            ..screen(0, 0, 1, 1, (0, 1))
        };
        let mut out = [0x77; 400];
        assert_eq!(encode(&frame, &mut out), Err(ERR_NO_SPACE));
        assert!(out.iter().all(|byte| *byte == 0x77));
        let mut small = [0x77; 4];
        assert_eq!(
            encode(&screen(0, 0, 1, 1, (0, 1)), &mut small),
            Err(ERR_NO_SPACE)
        );
        assert!(small.iter().all(|byte| *byte == 0x77));
    }

    #[test]
    fn purposes_have_their_contract_values() {
        let all = [
            Purpose::Screen,
            Purpose::Layer,
            Purpose::Insert,
            Purpose::Status,
            Purpose::Widget,
        ];
        assert_eq!(all.map(Purpose::wire), [0, 1, 2, 3, 4]);
    }
}
