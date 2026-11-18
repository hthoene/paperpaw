//! The bands of a frame as they were sent to the panel, kept packed (runs
//! of a byte, and bytes as they are between): after every refresh the
//! controller needs the frame now shown in both its memories, and sending
//! these again takes milliseconds where drawing the frame anew takes
//! seconds. Pages of text pack to a few kilobytes; pictures, dithered,
//! hardly pack, but take no more than the frame (48 KB) either. A frame
//! with memory short is drawn anew instead.

use alloc::vec::Vec;

use paperpaw_platform::FrameView;

/// What the kept bands may take of the heap: a whole frame, packed.
const MAX_BYTES: usize = 56 * 1024;

/// What is left free of the heap for the rest of the frame's drawing.
const SPARE: usize = 40 * 1024;

/// A band: its first row, its rows, its format and bytes per row, and its
/// bytes packed: a byte below 0x80 says that many bytes and one more
/// follow as they are; one from 0x80 on, that the next byte comes as
/// many times as its low 7 bits say, and three more.
#[derive(Debug)]
struct Band {
    top: u16,
    view: (u16, u16, paperpaw_platform::PixelFormat, usize),
    packed: Vec<u8>,
}

impl Band {
    /// Its bytes unpacked.
    fn bytes(&self) -> usize {
        usize::from(self.view.1) * self.view.3
    }
}

#[derive(Debug)]
pub struct Sent {
    /// The heap's free bytes now.
    heap_free: fn() -> usize,
    bands: Vec<Band>,
    size: usize,
    /// It did not fit: the frame is drawn anew.
    lost: bool,
}

/// Where a band's packed bytes go: onto the bytes kept, or nowhere when
/// they are only counted (`Count`), so that a band is kept in memory of
/// exactly its packed size and no second band's worth is held for packing.
trait Packed {
    fn push(&mut self, byte: u8);
    fn extend_from_slice(&mut self, bytes: &[u8]);
}

impl Packed for Vec<u8> {
    fn push(&mut self, byte: u8) {
        Vec::push(self, byte);
    }
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        Vec::extend_from_slice(self, bytes);
    }
}

/// How many bytes went in.
struct Count(usize);

impl Packed for Count {
    fn push(&mut self, _: u8) {
        self.0 += 1;
    }
    fn extend_from_slice(&mut self, bytes: &[u8]) {
        self.0 += bytes.len();
    }
}

/// Packs `bytes` onto `out`. A frame is packed twice, to count and to keep,
/// so the bytes are walked without a check at each (#248).
fn pack(bytes: &[u8], out: &mut impl Packed) {
    let mut rest = bytes;
    while let Some(&byte) = rest.first() {
        // A run is 130 bytes at most; so are the bytes that tell where the
        // bytes between runs end (128 of them and two more).
        let ahead = &rest[..rest.len().min(130)];
        let run = ahead.iter().position(|&b| b != byte).unwrap_or(ahead.len());
        // Runs of three on (shorter ones cost more as runs than as bytes
        // between them).
        let len = if run >= 3 {
            out.push(0x80 | (run - 3) as u8);
            out.push(byte);
            run
        } else {
            // As they are, until a run starts.
            let mut len = ahead.len().min(128);
            let mut same = 1;
            for (at, (a, b)) in ahead.iter().zip(&ahead[1..]).enumerate() {
                if a != b {
                    same = 1;
                } else if same == 2 {
                    // `b` is the third alike: the run started before `a`.
                    len = at.saturating_sub(1).max(1);
                    break;
                } else {
                    same = 2;
                }
            }
            out.push((len - 1) as u8);
            out.extend_from_slice(&rest[..len]);
            len
        };
        rest = &rest[len..];
    }
}

/// Unpacks `packed` onto `out`.
fn unpack(packed: &[u8], out: &mut Vec<u8>) {
    let mut at = 0;
    while let Some(&head) = packed.get(at) {
        if head < 0x80 {
            let end = (at + 2 + usize::from(head)).min(packed.len());
            out.extend_from_slice(&packed[at + 1..end]);
            at = end;
        } else {
            let byte = packed.get(at + 1).copied().unwrap_or(0xFF);
            out.extend(core::iter::repeat_n(byte, usize::from(head & 0x7F) + 3));
            at += 2;
        }
    }
}

impl Sent {
    /// Nothing kept yet; `heap_free` tells the heap's free bytes, so that
    /// keeping leaves enough of it for drawing the rest of the frame.
    pub fn new(heap_free: fn() -> usize) -> Self {
        Self {
            heap_free,
            bands: Vec::new(),
            size: 0,
            lost: false,
        }
    }

    /// Keeps a band as it is sent.
    pub fn keep(&mut self, top: u16, rows: &FrameView<'_>) {
        if self.lost {
            return;
        }
        let bytes = &rows.data[..(usize::from(rows.height) * rows.stride).min(rows.data.len())];
        // Counted first, then packed into memory of that size: nothing of
        // a band's size is held beside what is kept (#248).
        let mut count = Count(0);
        pack(bytes, &mut count);
        let size = count.0;
        let mut packed = Vec::new();
        if self.size + size > MAX_BYTES
            || (self.heap_free)() < size + SPARE
            || packed.try_reserve_exact(size).is_err()
            || self.bands.try_reserve(1).is_err()
        {
            return self.lose();
        }
        pack(bytes, &mut packed);
        self.size += size;
        self.bands.push(Band {
            top,
            view: (rows.width, rows.height, rows.format, rows.stride),
            packed,
        });
    }

    /// Whether the bands did not all fit: the frame is to be drawn anew.
    pub fn lost(&self) -> bool {
        self.lost
    }

    fn lose(&mut self) {
        self.lost = true;
        self.bands = Vec::new();
        self.size = 0;
    }

    /// Passes every band kept, as it was sent; `false` if they were not
    /// all kept (the frame is to be drawn anew).
    pub fn replay(&mut self, out: &mut dyn FnMut(u16, FrameView<'_>)) -> bool {
        if self.lost {
            return false;
        }
        // A band's memory for unpacking, taken once for the tallest before
        // any band goes out: without it nothing was sent twice.
        let most = self.bands.iter().map(Band::bytes).max().unwrap_or(0);
        let mut data = Vec::new();
        if data.try_reserve_exact(most).is_err() {
            return false;
        }
        for band in &self.bands {
            let (width, height, format, stride) = band.view;
            data.clear();
            unpack(&band.packed, &mut data);
            out(
                band.top,
                FrameView::new(width, height, format, stride, &data),
            );
        }
        true
    }

    /// As [`replay`](Sent::replay), for a frame whose memory is not drawn anew
    /// by the kernel (the busy mark: the kernel went on meanwhile). Where the
    /// bands cannot all be sent again, the panel's two memories differ over what
    /// is shown: `stale` is set, and the next frame drives every pixel. A flag
    /// already set stays. `true` if they were kept.
    pub fn replay_or_stale(
        &mut self,
        out: &mut dyn FnMut(u16, FrameView<'_>),
        stale: &mut bool,
    ) -> bool {
        let kept = self.replay(out);
        if !kept {
            *stale = true;
        }
        kept
    }
}

/// A check of the rows sent to a memory of the controller: their tops,
/// count and bytes (FNV-1a), to compare what was sent twice.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Check {
    rows: u32,
    hash: u32,
}

impl Check {
    /// Adds a band sent from row `top`.
    pub fn add(&mut self, top: u16, rows: &FrameView<'_>) {
        let bytes = rows
            .data
            .get(..usize::from(rows.height) * rows.stride)
            .unwrap_or(&[]);
        let mut hash = if self.hash == 0 {
            0x811C_9DC5
        } else {
            self.hash
        };
        for byte in top.to_le_bytes().iter().chain(bytes) {
            hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
        }
        self.hash = hash;
        self.rows += u32::from(rows.height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_platform::PixelFormat;

    fn plenty() -> usize {
        usize::MAX / 2
    }

    fn scarce() -> usize {
        0
    }

    fn band(data: &[u8]) -> FrameView<'_> {
        FrameView::new(80, (data.len() / 10) as u16, PixelFormat::Mono1, 10, data)
    }

    #[test]
    fn packs_and_unpacks_alike() {
        let cases: [&[u8]; 5] = [
            &[],
            &[1, 2],
            &[5; 300],
            &[1, 1, 2, 2, 2, 3, 4, 4, 4, 4, 0xFF],
            &[0x80; 131],
        ];
        for bytes in cases {
            let mut packed = Vec::new();
            pack(bytes, &mut packed);
            let mut unpacked = Vec::new();
            unpack(&packed, &mut unpacked);
            assert_eq!(unpacked, bytes);
        }
        let mixed: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 13) as u8).collect();
        let mut packed = Vec::new();
        pack(&mixed, &mut packed);
        assert!(packed.len() <= mixed.len() + mixed.len() / 128 + 1);
        let mut unpacked = Vec::new();
        unpack(&packed, &mut unpacked);
        assert_eq!(unpacked, mixed);
    }

    /// A band as a page of text packs it: white, with rows of mixed bytes.
    fn text_band() -> Vec<u8> {
        (0..9_600u32)
            .map(|i| {
                if i / 60 % 8 < 3 && i % 60 > 6 && i % 60 < 52 {
                    (i.wrapping_mul(2_654_435_761) >> 13) as u8
                } else {
                    0xFF
                }
            })
            .collect()
    }

    /// Packing as it was before a band was counted and then packed (#248):
    /// what the bytes kept are compared with.
    fn pack_as_before(bytes: &[u8], out: &mut Vec<u8>) {
        let run_at = |at: usize| {
            bytes.get(at + 1) == Some(&bytes[at]) && bytes.get(at + 2) == Some(&bytes[at])
        };
        let mut at = 0;
        while at < bytes.len() {
            let byte = bytes[at];
            let run = bytes[at..]
                .iter()
                .take(130)
                .take_while(|&&b| b == byte)
                .count();
            if run >= 3 {
                out.push(0x80 | (run - 3) as u8);
                out.push(byte);
                at += run;
            } else {
                let start = at;
                at += 1;
                while at < bytes.len() && at - start < 128 && !run_at(at) {
                    at += 1;
                }
                out.push((at - start - 1) as u8);
                out.extend_from_slice(&bytes[start..at]);
            }
        }
    }

    #[test]
    fn packs_to_the_bytes_it_always_packed_to() {
        // Bytes alike in runs of every length around the limits (3, 128,
        // 130), single bytes, and mixes of them.
        let mut x: u32 = 12_345;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        for case in 0..4_000u32 {
            let len = (next() % 700) as usize;
            let mut bytes = Vec::with_capacity(len);
            while bytes.len() < len {
                let byte = (next() % if case % 5 == 0 { 256 } else { 3 }) as u8;
                let run = match case % 5 {
                    0 => 1,
                    1 => 1 + next() % 4,
                    2 => 1 + next() % 140,
                    3 => 1 + next() % 3,
                    _ => 120 + next() % 20,
                } as usize;
                bytes.resize(bytes.len() + run.min(len - bytes.len()), byte);
            }
            let (mut before, mut now, mut count) = (Vec::new(), Vec::new(), Count(0));
            pack_as_before(&bytes, &mut before);
            pack(&bytes, &mut now);
            pack(&bytes, &mut count);
            assert_eq!(now, before, "case {case}: {bytes:?}");
            assert_eq!(count.0, before.len(), "case {case}");
        }
        let pages: [Vec<u8>; 3] = [text_band(), [0xFF; 9_600].to_vec(), (0..=255).collect()];
        for bytes in pages {
            let (mut before, mut now) = (Vec::new(), Vec::new());
            pack_as_before(&bytes, &mut before);
            pack(&bytes, &mut now);
            assert_eq!(now, before);
        }
    }

    #[test]
    fn bytes_up_to_the_look_ahead_then_a_run_pack_as_they_always_did() {
        // Bytes with no three alike, as many as the look-ahead takes at once
        // or one or two fewer or more, then a run: one that ends the data or
        // not, of a byte that is the last one before it or another.
        let plain = |n: usize| -> Vec<u8> { (0..n).map(|i| 1 + (i % 2) as u8).collect() };
        for n in 124..=132 {
            for run in [0, 3, 4, 130, 131] {
                for byte in [9, plain(n)[n - 1]] {
                    for after in [0, 1, 2, 5] {
                        let mut bytes = plain(n);
                        bytes.resize(n + run, byte);
                        bytes.extend((0..after).map(|i| 20 + (i % 2) as u8));
                        let (mut before, mut now, mut count) = (Vec::new(), Vec::new(), Count(0));
                        pack_as_before(&bytes, &mut before);
                        pack(&bytes, &mut now);
                        pack(&bytes, &mut count);
                        assert_eq!(now, before, "{n} plain, {run} of {byte}, {after} after");
                        assert_eq!(count.0, before.len());
                        let mut unpacked = Vec::new();
                        unpack(&now, &mut unpacked);
                        assert_eq!(unpacked, bytes);
                    }
                }
            }
        }
        // And as bytes: 126, 127 and 128 of them go as one piece before a run
        // of three that ends the data; 129 as 128 and one.
        for n in [126usize, 127, 128] {
            let mut bytes = plain(n);
            bytes.resize(n + 3, 9);
            let mut packed = Vec::new();
            pack(&bytes, &mut packed);
            let mut expected = Vec::from([(n - 1) as u8]);
            expected.extend_from_slice(&bytes[..n]);
            expected.extend_from_slice(&[0x80, 9]);
            assert_eq!(packed, expected, "{n}");
        }
        let mut bytes = plain(129);
        bytes.resize(132, 9);
        let mut packed = Vec::new();
        pack(&bytes, &mut packed);
        let mut expected = Vec::from([127]);
        expected.extend_from_slice(&bytes[..128]);
        expected.extend_from_slice(&[0, bytes[128], 0x80, 9]);
        assert_eq!(packed, expected);
    }

    #[test]
    fn counting_says_the_size_packing_takes() {
        let noise: Vec<u8> = (0..9_600u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 11) as u8)
            .collect();
        let runs: Vec<u8> = (0..2_000u32).map(|i| (i / 131) as u8).collect();
        let cases: [&[u8]; 8] = [
            &[],
            &[7],
            &[1, 1],
            &[5; 130],
            &[5; 131],
            &runs,
            &noise,
            &text_band(),
        ];
        for bytes in cases {
            let mut packed = Vec::new();
            pack(bytes, &mut packed);
            let mut count = Count(0);
            pack(bytes, &mut count);
            assert_eq!(count.0, packed.len());
        }
    }

    #[test]
    fn a_band_is_kept_as_it_packs_in_memory_of_that_size() {
        // The bytes a band packs to, as they have been since the bands
        // were first kept.
        let bytes = [
            1, 1, 2, 2, 2, 3, 4, 4, 4, 4, 0xFF, 9, 9, 9, 9, 9, 9, 9, 9, 9,
        ];
        let mut sent = Sent::new(plenty);
        sent.keep(0, &band(&bytes));
        let kept = &sent.bands[0].packed;
        assert_eq!(
            kept[..],
            [1, 1, 1, 0x80, 2, 0, 3, 0x81, 4, 0, 0xFF, 0x86, 9]
        );
        assert_eq!(kept.capacity(), kept.len());
        // And a band of a page, against packing it whole.
        let text = text_band();
        let mut whole = Vec::new();
        pack(&text, &mut whole);
        let mut sent = Sent::new(plenty);
        sent.keep(0, &FrameView::new(480, 160, PixelFormat::Mono1, 60, &text));
        assert_eq!(sent.bands[0].packed, whole);
        assert_eq!(sent.size, whole.len());
    }

    #[test]
    fn replays_the_bands_as_sent() {
        let first: Vec<u8> = (0..40).collect();
        let second = [0xFF; 20];
        let mut sent = Sent::new(plenty);
        sent.keep(0, &band(&first));
        sent.keep(4, &band(&second));
        let (mut again, mut check, mut check_again) =
            (Vec::new(), Check::default(), Check::default());
        check.add(0, &band(&first));
        check.add(4, &band(&second));
        assert!(sent.replay(&mut |top, rows| {
            again.push((top, rows.data.to_vec()));
            check_again.add(top, &rows);
        }));
        assert_eq!(again, [(0, first.clone()), (4, second.to_vec())]);
        assert_eq!(check, check_again);
    }

    #[test]
    fn short_of_memory_it_draws_anew() {
        let mut sent = Sent::new(|| 1024);
        sent.keep(0, &band(&[0; 40]));
        assert!(!sent.replay(&mut |_, _| {}));
    }

    #[test]
    fn a_refused_replay_marks_the_next_frame_to_be_shown_whole() {
        let data = [7u8; 100];
        let mut sent = Sent::new(scarce);
        sent.keep(0, &band(&data));
        let (mut stale, mut passed) = (false, 0);
        assert!(!sent.replay_or_stale(&mut |_, _| passed += 1, &mut stale));
        assert!(
            stale,
            "the memories differ: the next frame drives every pixel"
        );
        assert_eq!(passed, 0);
    }

    #[test]
    fn a_kept_replay_leaves_the_next_frame_as_it_was() {
        let data = [7u8; 100];
        for before in [false, true] {
            let mut sent = Sent::new(plenty);
            sent.keep(0, &band(&data));
            let (mut stale, mut passed) = (before, 0);
            assert!(sent.replay_or_stale(&mut |_, _| passed += 1, &mut stale));
            assert_eq!((stale, passed), (before, 1));
        }
    }
}
