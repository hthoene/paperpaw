//! The cable link (USB): the computer at the other end of the cable
//! reaches the server a plugin runs on
//! [`CABLE_PORT`](paperpaw_sdk::served::CABLE_PORT) with the same
//! requests as over the network, framed as lines of text (protocol 1,
//! `docs/usb-transfer.md`). The shell only passes the lines on
//! (`Kernel::cable_line`, `Kernel::take_cable_lines`); a line that is not
//! the cable's is passed over, so the cable shares the serial link with a
//! log or a test's remote control.
//!
//! - The computer: `PPT/1 <n> <METHOD> <target> <body bytes>`, then the
//!   body as `PPT/1 <n> data <hex>` lines of at most [`DATA`] bytes, until
//!   all of it came. `<n>` is the computer's number for the request.
//! - The device: `PPT/1 <n> <status> <content type>`, the answer's body as
//!   `PPT/1 <n> data <hex>` lines of at most [`DATA`] bytes, then
//!   `PPT/1 <n> end`.
//!
//! One request at a time has a body coming. Answers that would send a
//! file (`body_from`) are refused: nothing is read from the device over
//! the cable but what a plugin puts in its answers.

use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// What a line of the cable begins with.
pub const PREFIX: &str = "PPT/1 ";
/// Bytes of a body in one `data` line at most.
pub const DATA: usize = 512;
/// The kernel's numbers for the cable's requests begin here, apart from
/// those a shell gives requests over the network.
pub(crate) const FIRST_REQUEST: u32 = 0x8000_0000;

/// A line of the computer's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Line {
    Request {
        n: u32,
        method: String,
        target: String,
        len: u64,
    },
    Data {
        n: u32,
        bytes: Vec<u8>,
    },
    /// The cable's, but not understood: why (answered with *400* where it
    /// names a request).
    Bad {
        n: Option<u32>,
        why: &'static str,
    },
}

/// The line read, if it is the cable's.
pub(crate) fn parse(line: &str) -> Option<Line> {
    let rest = line.trim_end().strip_prefix(PREFIX)?;
    let mut words = rest.split(' ').filter(|w| !w.is_empty());
    let Some(n) = words.next().and_then(|n| n.parse::<u32>().ok()) else {
        return Some(Line::Bad {
            n: None,
            why: "no request number",
        });
    };
    let bad = |why| Some(Line::Bad { n: Some(n), why });
    match words.next() {
        Some("data") => match (words.next(), words.next()) {
            // Longer than `DATA` bytes of digits is refused before anything is reserved.
            (Some(digits), None) if digits.len() <= DATA * 2 => {
                match paperpaw_sdk::digest::hex::try_decode(digits) {
                    Ok(Some(bytes)) => Some(Line::Data { n, bytes }),
                    Ok(None) => bad("bad data"),
                    Err(_) => bad("out of memory"),
                }
            }
            _ => bad("bad data"),
        },
        Some(method) if is_method(method) => {
            let (Some(target), Some(len), None) = (words.next(), words.next(), words.next()) else {
                return bad("a request is: <n> <METHOD> <target> <body bytes>");
            };
            let Ok(len) = len.parse() else {
                return bad("bad body length");
            };
            if !target.starts_with('/') {
                return bad("the target begins with /");
            }
            Some(Line::Request {
                n,
                method: String::from(method),
                target: String::from(target),
                len,
            })
        }
        _ => bad("no method"),
    }
}

fn is_method(word: &str) -> bool {
    !word.is_empty() && word.len() <= 8 && word.bytes().all(|b| b.is_ascii_uppercase())
}

/// After *Back* on its screen, the cable is opened again only by a
/// request after a pause this long: one asking all the while (a client
/// waiting for an answer) does not bring it back.
pub(crate) const PAUSE_MS: u64 = 10_000;

/// An answer being sent, a line at a time.
#[derive(Debug)]
struct Sending {
    n: u32,
    /// Its first line, until sent.
    head: Option<String>,
    body: Vec<u8>,
    /// How much of the body went.
    at: usize,
}

/// The requests under way over the cable, and the answers to send.
#[derive(Debug, Default)]
pub(crate) struct Cable {
    /// The computer's number and the kernel's of each request not
    /// answered yet.
    open: Vec<(u32, u32)>,
    /// The request whose body comes: the computer's number, the kernel's,
    /// bytes to come.
    body: Option<(u32, u32, u64)>,
    next: u32,
    out: VecDeque<Sending>,
    /// The reader ended it on the device (the screen closed).
    pub(crate) ended: bool,
    /// When the computer's last line came.
    pub(crate) last: Option<u64>,
}

impl Cable {
    /// A request numbered `n` by the computer, with `len` bytes of body to
    /// come: the kernel's number for it, and the request whose body was
    /// still coming, cut short by it (its computer's number and the
    /// kernel's), if any.
    pub(crate) fn begin(&mut self, n: u32, len: u64) -> (u32, Option<(u32, u32)>) {
        let cut = self.body.take().map(|(m, request, _)| (m, request));
        if let Some((m, _)) = cut {
            self.open.retain(|(o, _)| *o != m);
        }
        self.forget(n);
        let request = FIRST_REQUEST | (self.next & !FIRST_REQUEST);
        self.next = self.next.wrapping_add(1);
        self.open.push((n, request));
        self.body = (len > 0).then_some((n, request, len));
        (request, cut)
    }

    /// Part of the body of request `n`: the kernel's number for it, and
    /// whether it is the last; `None` where no body of `n` is coming or
    /// the part is longer than what was said.
    pub(crate) fn data(&mut self, n: u32, len: usize) -> Option<(u32, bool)> {
        let (m, request, left) = self.body?;
        if m != n || len as u64 > left {
            return None;
        }
        let left = left - len as u64;
        self.body = (left > 0).then_some((n, request, left));
        Some((request, left == 0))
    }

    fn request(&self, n: u32) -> Option<u32> {
        self.open.iter().find(|(m, _)| *m == n).map(|(_, r)| *r)
    }

    /// Forgets request `n` (a new one by that number, or it went wrong).
    fn forget(&mut self, n: u32) {
        if self.body.is_some_and(|(m, _, _)| m == n) {
            self.body = None;
        }
        self.open.retain(|(m, _)| *m != n);
    }

    /// The kernel's number of request `n`, whose body is cut short: the
    /// request is forgotten.
    pub(crate) fn cut(&mut self, n: u32) -> Option<u32> {
        let request = self.request(n);
        self.forget(n);
        request
    }

    /// Answers request `n` with `status` and `why`, without its plugin.
    pub(crate) fn refuse(&mut self, n: u32, status: u16, why: &str) {
        self.forget(n);
        self.send(
            n,
            status,
            "text/plain; charset=utf-8",
            why.as_bytes().to_vec(),
        );
    }

    /// Answers the request the kernel numbered `request` with `status` and
    /// `why`, without its plugin; nothing where it is not open.
    pub(crate) fn refuse_request(&mut self, request: u32, status: u16, why: &str) {
        if let Some(&(n, _)) = self.open.iter().find(|(_, r)| *r == request) {
            self.refuse(n, status, why);
        }
    }

    /// The plugin's answer to the kernel's request `request`, if it is
    /// the cable's; a file to send is refused.
    pub(crate) fn answer(
        &mut self,
        request: u32,
        status: u16,
        content_type: &str,
        body: Vec<u8>,
        file: bool,
    ) {
        let Some(&(n, _)) = self.open.iter().find(|(_, r)| *r == request) else {
            return;
        };
        self.forget(n);
        if file {
            let why = b"files are not sent over the cable".to_vec();
            self.send(n, 500, "text/plain; charset=utf-8", why);
        } else {
            self.send(n, status, content_type, body);
        }
    }

    fn send(&mut self, n: u32, status: u16, content_type: &str, body: Vec<u8>) {
        let content_type = content_type.replace(' ', "");
        self.out.push_back(Sending {
            n,
            head: Some(format!("{PREFIX}{n} {status} {content_type}")),
            body,
            at: 0,
        });
    }

    /// The next line to send to the computer: answers go a line at a
    /// time, each part of a body written out only as it goes.
    pub(crate) fn next_line(&mut self) -> Option<String> {
        let sending = self.out.front_mut()?;
        if let Some(head) = sending.head.take() {
            return Some(head);
        }
        let n = sending.n;
        if sending.at < sending.body.len() {
            let end = (sending.at + DATA).min(sending.body.len());
            let part = &sending.body[sending.at..end];
            sending.at = end;
            return Some(format!(
                "{PREFIX}{n} data {}",
                paperpaw_sdk::digest::hex::encode(part)
            ));
        }
        self.out.pop_front();
        Some(format!("{PREFIX}{n} end"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn take(cable: &mut Cable) -> Vec<String> {
        core::iter::from_fn(|| cable.next_line()).collect()
    }

    /// A request coming while another's body still comes cuts that one
    /// short (it is told, its body let go), even one without a body.
    #[test]
    fn a_new_request_cuts_a_body_still_coming() {
        let mut cable = Cable::default();
        let (first, cut) = cable.begin(1, 100);
        assert_eq!(cut, None);
        assert_eq!(cable.data(1, 10), Some((first, false)));
        let (_, cut) = cable.begin(2, 0);
        assert_eq!(cut, Some((1, first)));
        assert_eq!(cable.data(1, 10), None);
    }

    /// A data line holds at most `DATA` bytes; a longer one is refused as it
    /// is read, and one with more words is too.
    #[test]
    fn a_data_line_is_cut_at_its_size_and_words() {
        let full = alloc::format!("PPT/1 7 data {}", "ab".repeat(DATA));
        assert!(matches!(parse(&full), Some(Line::Data { n: 7, bytes }) if bytes.len() == DATA));
        for bad in [
            alloc::format!("{full}ab"),
            alloc::format!("{full} 00"),
            String::from("PPT/1 7 data"),
            String::from("PPT/1 7 data 00 00"),
        ] {
            assert!(
                matches!(parse(&bad), Some(Line::Bad { n: Some(7), .. })),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn lines_are_read_as_the_protocol_says() {
        assert_eq!(parse("hello"), None);
        assert_eq!(parse("PP:OK"), None);
        assert_eq!(
            parse("PPT/1 7 PUT /api/book/a.epub?sha256=00 12\r\n"),
            Some(Line::Request {
                n: 7,
                method: String::from("PUT"),
                target: String::from("/api/book/a.epub?sha256=00"),
                len: 12,
            })
        );
        assert_eq!(
            parse("PPT/1 7 data 00ff"),
            Some(Line::Data {
                n: 7,
                bytes: alloc::vec![0, 255],
            })
        );
        for bad in [
            "PPT/1 x GET / 0",
            "PPT/1 7 data 0",
            "PPT/1 7 data zz",
            "PPT/1 7 get / 0",
            "PPT/1 7 GET api 0",
            "PPT/1 7 GET / -1",
            "PPT/1 7 GET / 0 more",
        ] {
            assert!(matches!(parse(bad), Some(Line::Bad { .. })), "{bad}");
        }
        let long = format!("PPT/1 1 data {}", "00".repeat(DATA + 1));
        assert!(matches!(parse(&long), Some(Line::Bad { .. })));
    }

    /// A body is taken as long as it was said, no more; answers go in
    /// parts of [`DATA`] bytes, and files are not sent.
    #[test]
    fn bodies_come_and_answers_go_in_parts() {
        let mut cable = Cable::default();
        let (request, _) = cable.begin(3, 5);
        assert!(request >= FIRST_REQUEST);
        assert_eq!(cable.data(3, 3), Some((request, false)));
        assert_eq!(cable.data(4, 1), None);
        assert_eq!(cable.data(3, 3), None);
        assert_eq!(cable.data(3, 2), Some((request, true)));
        assert_eq!(cable.data(3, 1), None);
        cable.answer(
            request,
            200,
            "application/json",
            vec![b'x'; DATA + 1],
            false,
        );
        let lines = take(&mut cable);
        assert_eq!(lines[0], "PPT/1 3 200 application/json");
        assert_eq!(lines[1].len(), "PPT/1 3 data ".len() + 2 * DATA);
        assert_eq!(lines[2], "PPT/1 3 data 78");
        assert_eq!(lines[3], "PPT/1 3 end");
        // Answered once only.
        cable.answer(request, 200, "text/plain", b"again".to_vec(), false);
        assert!(take(&mut cable).is_empty());
        let (request, _) = cable.begin(4, 0);
        cable.answer(request, 200, "application/octet-stream", Vec::new(), true);
        let lines = take(&mut cable);
        assert_eq!(lines[0], "PPT/1 4 500 text/plain;charset=utf-8");
    }
}
