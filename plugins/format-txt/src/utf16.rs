//! Incremental UTF-16 conversion into caller-owned storage chunks.
//! The caller skips the BOM. A trailing odd byte is ignored, as in the parser.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Decoder {
    high: Option<u16>,
    byte: Option<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Progress {
    pub read: usize,
    pub written: usize,
    pub done: bool,
}

impl Decoder {
    /// A short output leaves the next unit unconsumed. Supply at least six
    /// output bytes to guarantee progress (two replacement/BMP characters).
    pub(crate) fn convert(
        &mut self,
        input: &[u8],
        out: &mut [u8],
        big: bool,
        last: bool,
    ) -> Progress {
        let mut read = 0;
        let mut written = 0;
        loop {
            let (pair, count) = match (self.byte, input.get(read..)) {
                (Some(a), Some([b, ..])) => ([a, *b], 1),
                (None, Some([a, b, ..])) => ([*a, *b], 2),
                (None, Some([a])) => {
                    self.byte = Some(*a);
                    read += 1;
                    break;
                }
                _ => break,
            };
            let unit = if big {
                u16::from_be_bytes(pair)
            } else {
                u16::from_le_bytes(pair)
            };
            let mut next = *self;
            next.byte = None;
            let mut chars = [None, None];
            match (next.high.take(), unit) {
                (Some(high), 0xdc00..=0xdfff) => {
                    let code =
                        0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(unit) - 0xdc00;
                    chars[0] = Some(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                (high, _) => {
                    chars[0] = high.map(|_| '\u{fffd}');
                    if (0xd800..=0xdbff).contains(&unit) {
                        next.high = Some(unit);
                    } else {
                        chars[1] = Some(char::from_u32(u32::from(unit)).unwrap_or('\u{fffd}'));
                    }
                }
            }
            let needed: usize = chars.iter().flatten().map(|c| c.len_utf8()).sum();
            if needed > out.len() - written {
                return Progress {
                    read,
                    written,
                    done: false,
                };
            }
            for ch in chars.into_iter().flatten() {
                let mut bytes = [0; 4];
                let text = ch.encode_utf8(&mut bytes);
                out[written..written + text.len()].copy_from_slice(text.as_bytes());
                written += text.len();
            }
            *self = next;
            read += count;
        }
        if last && read == input.len() {
            if self.high.is_some() {
                if out.len() - written < 3 {
                    return Progress {
                        read,
                        written,
                        done: false,
                    };
                }
                out[written..written + 3].copy_from_slice("\u{fffd}".as_bytes());
                written += 3;
            }
            self.high = None;
            self.byte = None;
            return Progress {
                read,
                written,
                done: true,
            };
        }
        Progress {
            read,
            written,
            done: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{string::String, vec::Vec};

    #[test]
    fn all_chunk_boundaries_preserve_surrogates_and_odd_tail() {
        let units = [0x41u16, 0xd83d, 0xde00, 0xd800, 0x42, 0xdc00, 0xd800];
        let expected: String = char::decode_utf16(units)
            .map(|r| r.unwrap_or('\u{fffd}'))
            .collect();
        for big in [false, true] {
            let mut bytes: Vec<u8> = units
                .iter()
                .flat_map(|u| {
                    if big {
                        u.to_be_bytes()
                    } else {
                        u.to_le_bytes()
                    }
                })
                .collect();
            bytes.push(0xff);
            for split in 0..=bytes.len() {
                let mut decoder = Decoder::default();
                let mut actual = Vec::new();
                for (part, last) in [(&bytes[..split], false), (&bytes[split..], true)] {
                    let mut at = 0;
                    loop {
                        let mut out = [0; 6];
                        let p = decoder.convert(&part[at..], &mut out, big, last);
                        actual.extend_from_slice(&out[..p.written]);
                        at += p.read;
                        if p.done || (!last && at == part.len()) {
                            break;
                        }
                        assert!(p.read != 0 || p.written != 0);
                    }
                }
                assert_eq!(actual, expected.as_bytes(), "split {split}, big {big}");
            }
        }
    }

    #[test]
    fn short_output_does_not_consume_a_unit_or_overwrite_output() {
        let mut decoder = Decoder::default();
        let original = decoder;
        let mut out = [0xa5; 1];
        assert_eq!(
            decoder.convert(&[0xac, 0x20], &mut out, false, true),
            Progress {
                read: 0,
                written: 0,
                done: false
            }
        );
        assert_eq!(decoder, original);
        assert_eq!(out, [0xa5]);
    }
}
