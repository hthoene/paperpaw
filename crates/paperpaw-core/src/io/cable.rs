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
            let (Some(method), Some(target)) = (own(method), own(target)) else {
                return bad("out of memory");
            };
            Some(Line::Request {
                n,
                method,
                target,
                len,
            })
        }
        _ => bad("no method"),
    }
}

/// `text` copied, fallibly (the line is the computer's; the shell caps it).
fn own(text: &str) -> Option<String> {
    let mut owned = String::new();
    owned.try_reserve_exact(text.len()).ok()?;
    owned.push_str(text);
    Some(owned)
}

fn is_method(word: &str) -> bool {
    !word.is_empty() && word.len() <= 8 && word.bytes().all(|b| b.is_ascii_uppercase())
}

/// After *Back* on its screen, the cable is opened again only by a
/// request after a pause this long: one asking all the while (a client
/// waiting for an answer) does not bring it back.
pub(crate) const PAUSE_MS: u64 = 10_000;

/// Answers waiting to be sent at most, and their bodies' bytes: a computer
/// that sends without reading cannot grow the queue past them (what does
/// not fit is not answered; the computer was told enough already).
pub(crate) const OUT_MAX: usize = 8;
/// See [`OUT_MAX`].
pub(crate) const OUT_BYTES_MAX: usize = 16 * 1024;

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
    /// The body bytes of the answers in `out`.
    out_bytes: usize,
    /// A refused request whose body still comes: the computer's number and
    /// the bytes to come, passed over without an answer each.
    swallow: Option<(u32, u64)>,
    /// The reader ended it on the device (the screen closed).
    pub(crate) ended: bool,
    /// When the computer's last line came.
    pub(crate) last: Option<u64>,
}

impl Cable {
    /// A request numbered `n` by the computer, with `len` bytes of body to
    /// come: the kernel's number for it, the request whose body was still
    /// coming, cut short by it (its computer's number and the kernel's), if
    /// any, and the kernel's number of an earlier request `n` still open
    /// without a body coming, if any: the computer gave that one up, so it
    /// ends (its plugin told) without an answer of its own, since an answer
    /// numbered `n` is the new request's.
    pub(crate) fn begin(&mut self, n: u32, len: u64) -> (u32, Option<(u32, u32)>, Option<u32>) {
        let cut = self.body.take().map(|(m, request, _)| (m, request));
        if let Some((m, _)) = cut {
            self.open.retain(|(o, _)| *o != m);
        }
        let replaced = self.request(n);
        self.forget(n);
        let request = FIRST_REQUEST | (self.next & !FIRST_REQUEST);
        self.next = self.next.wrapping_add(1);
        // Without room the request is not kept: its answer finds nothing.
        if self.open.try_reserve(1).is_ok() {
            self.open.push((n, request));
        }
        self.body = (len > 0).then_some((n, request, len));
        // A new request ends what was being passed over.
        self.swallow = None;
        (request, cut, replaced)
    }

    /// Whether the `len` bytes of a data line of `n` belong to a refused
    /// request's body: passed over, with no answer of their own.
    pub(crate) fn swallowed(&mut self, n: u32, len: usize) -> bool {
        let Some((m, left)) = self.swallow else {
            return false;
        };
        if m != n {
            return false;
        }
        let left = left.saturating_sub(len as u64);
        self.swallow = (left > 0).then_some((n, left));
        true
    }

    /// Answers request `n` that announced `len` bytes of body with
    /// `status` and `why` before any of it came: its data lines are passed
    /// over ([`Self::swallowed`]).
    pub(crate) fn refuse_upload(&mut self, n: u32, len: u64, status: u16, why: &str) {
        self.refuse(n, status, why);
        if len > 0 {
            self.swallow = Some((n, len));
        }
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

    /// Forgets request `n` (a new one by that number, or it went wrong); the
    /// rest of a body it still had coming is passed over.
    fn forget(&mut self, n: u32) {
        if let Some((m, _, left)) = self.body
            && m == n
        {
            self.body = None;
            self.swallow = Some((n, left));
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
        let mut body = Vec::new();
        if body.try_reserve_exact(why.len()).is_ok() {
            body.extend_from_slice(why.as_bytes());
        }
        self.send(n, status, "text/plain; charset=utf-8", body);
    }

    /// Answers the request the kernel numbered `request` with `status` and
    /// `why`, without its plugin; nothing where it is not open.
    pub(crate) fn refuse_request(&mut self, request: u32, status: u16, why: &str) {
        if let Some(&(n, _)) = self.open.iter().find(|(_, r)| *r == request) {
            self.refuse(n, status, why);
        }
    }

    /// The plugin's answer to the kernel's request `request`, if it is
    /// the cable's; a file to send is refused. `false` when it could not be
    /// queued (no memory, or the queue is full).
    pub(crate) fn answer(
        &mut self,
        request: u32,
        status: u16,
        content_type: &str,
        body: Vec<u8>,
        file: bool,
    ) -> bool {
        let Some(&(n, _)) = self.open.iter().find(|(_, r)| *r == request) else {
            return false;
        };
        self.forget(n);
        if file {
            const WHY: &[u8] = b"files are not sent over the cable";
            let mut why = Vec::new();
            if why.try_reserve_exact(WHY.len()).is_ok() {
                why.extend_from_slice(WHY);
            }
            self.send(n, 500, "text/plain; charset=utf-8", why)
        } else {
            self.send(n, status, content_type, body)
        }
    }

    /// Whether an answer with `len` bytes of body is queued now: fewer than
    /// [`OUT_MAX`] wait and their bodies stay within [`OUT_BYTES_MAX`]; one
    /// alone is queued at any size (an interface-6 listing can be longer).
    pub(crate) fn has_room(&self, len: usize) -> bool {
        self.out.len() < OUT_MAX
            && (self.out.is_empty() || self.out_bytes.saturating_add(len) <= OUT_BYTES_MAX)
    }

    /// Queues an answer; `false` (nothing queued) without room
    /// ([`Self::has_room`]) or memory. Every allocation is fallible: the
    /// computer at the cable is an outside party.
    fn send(&mut self, n: u32, status: u16, content_type: &str, body: Vec<u8>) -> bool {
        use core::fmt::Write;
        if !self.has_room(body.len()) {
            return false;
        }
        let mut head = String::new();
        // "PPT/1 " + number + status + the type without spaces.
        if head
            .try_reserve_exact(PREFIX.len() + 10 + 1 + 3 + 1 + content_type.len())
            .is_err()
            || self.out.try_reserve(1).is_err()
        {
            return false;
        }
        let _ = write!(head, "{PREFIX}{n} {status} ");
        head.extend(content_type.chars().filter(|c| *c != ' '));
        self.out_bytes += body.len();
        self.out.push_back(Sending {
            n,
            head: Some(head),
            body,
            at: 0,
        });
        true
    }

    /// The next line to send to the computer: answers go a line at a
    /// time, each part of a body written out only as it goes.
    /// `None` also when the line cannot be allocated now: it is tried
    /// again at the next call, nothing is lost or sent twice.
    pub(crate) fn next_line(&mut self) -> Option<String> {
        use core::fmt::Write;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let sending = self.out.front_mut()?;
        if let Some(head) = sending.head.take() {
            return Some(head);
        }
        let n = sending.n;
        let mut line = String::new();
        if sending.at < sending.body.len() {
            let end = (sending.at + DATA).min(sending.body.len());
            let part = &sending.body[sending.at..end];
            // "PPT/1 " + number + " data " + two digits a byte.
            line.try_reserve_exact(PREFIX.len() + 10 + 6 + part.len() * 2)
                .ok()?;
            let _ = write!(line, "{PREFIX}{n} data ");
            for byte in part {
                line.push(char::from(HEX[usize::from(byte >> 4)]));
                line.push(char::from(HEX[usize::from(byte & 0xf)]));
            }
            sending.at = end;
            return Some(line);
        }
        line.try_reserve_exact(PREFIX.len() + 10 + 4).ok()?;
        let _ = write!(line, "{PREFIX}{n} end");
        if let Some(done) = self.out.pop_front() {
            self.out_bytes = self.out_bytes.saturating_sub(done.body.len());
        }
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{format, vec};

    fn take(cable: &mut Cable) -> Vec<String> {
        core::iter::from_fn(|| cable.next_line()).collect()
    }

    /// The rest of a refused body is passed over: no answer per line.
    #[test]
    fn a_refused_body_is_passed_over_without_an_answer_a_line() {
        let mut cable = Cable::default();
        cable.refuse_upload(1, 1_000, 503, "nothing serves the cable");
        assert_eq!(take(&mut cable).len(), 3, "one answer");
        for _ in 0..1_000 / 500 {
            assert!(cable.swallowed(1, 500));
        }
        assert!(!cable.swallowed(1, 1), "all of it came");
        // A body refused after it began (the server's limit) too.
        let (request, _, _) = cable.begin(2, 1_000);
        assert_eq!(cable.data(2, 100), Some((request, false)));
        cable.refuse_request(request, 413, "");
        let _ = take(&mut cable);
        assert!(cable.swallowed(2, 900));
        // A new request ends the passing over.
        cable.refuse_upload(3, 100, 503, "");
        cable.begin(4, 0);
        assert!(!cable.swallowed(3, 100));
    }

    /// A computer that sends without reading cannot grow the queue.
    #[test]
    fn the_answers_waiting_are_bounded() {
        let mut cable = Cable::default();
        for n in 0..100 {
            cable.refuse(n, 400, "bad");
        }
        assert_eq!(cable.out.len(), OUT_MAX);
        let (request, _, _) = cable.begin(200, 0);
        let big = vec![0u8; OUT_BYTES_MAX];
        assert!(!cable.answer(request, 200, "text/plain", big, false));
        let sent = take(&mut cable);
        assert_eq!(sent.len(), OUT_MAX * 3, "head, data, end each");
        assert_eq!(cable.out_bytes, 0);
    }

    /// Alone, an answer of any size is sent (a long interface-6 listing);
    /// behind another, one past the bytes waits for room.
    #[test]
    fn one_answer_alone_is_sent_at_any_size() {
        let mut cable = Cable::default();
        let (first, _, _) = cable.begin(1, 0);
        let (second, _, _) = cable.begin(2, 0);
        assert!(cable.has_room(OUT_BYTES_MAX + 1));
        let big = vec![b'x'; OUT_BYTES_MAX + 1];
        assert!(cable.answer(first, 200, "application/json", big, false));
        assert!(!cable.has_room(1), "the bytes are taken");
        let lines = take(&mut cable);
        assert_eq!(lines.len(), 2 + (OUT_BYTES_MAX + 1).div_ceil(DATA));
        assert_eq!(lines.last().map(String::as_str), Some("PPT/1 1 end"));
        assert!(cable.has_room(OUT_BYTES_MAX));
        assert!(cable.answer(second, 200, "text/plain", vec![b'y'; 10], false));
    }

    /// A request number the computer uses again ends the earlier request
    /// still open by it: it is reported, and not answered (an answer by that
    /// number is the new one's).
    #[test]
    fn a_number_used_again_ends_the_earlier_request() {
        let mut cable = Cable::default();
        let (first, _, replaced) = cable.begin(5, 0);
        assert_eq!(replaced, None);
        let (second, cut, replaced) = cable.begin(5, 0);
        assert_eq!((cut, replaced), (None, Some(first)));
        assert!(!cable.answer(first, 200, "text/plain", b"old".to_vec(), false));
        assert!(cable.answer(second, 200, "text/plain", b"new".to_vec(), false));
        assert_eq!(take(&mut cable)[1], "PPT/1 5 data 6e6577");
        // One whose body still comes is the cut one, not reported twice.
        let (third, _, _) = cable.begin(6, 10);
        let (_, cut, replaced) = cable.begin(6, 0);
        assert_eq!((cut, replaced), (Some((6, third)), None));
    }

    /// A request coming while another's body still comes cuts that one
    /// short (it is told, its body let go), even one without a body.
    #[test]
    fn a_new_request_cuts_a_body_still_coming() {
        let mut cable = Cable::default();
        let (first, cut, _) = cable.begin(1, 100);
        assert_eq!(cut, None);
        assert_eq!(cable.data(1, 10), Some((first, false)));
        let (_, cut, _) = cable.begin(2, 0);
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
        let (request, _, _) = cable.begin(3, 5);
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
        let (request, _, _) = cable.begin(4, 0);
        cable.answer(request, 200, "application/octet-stream", Vec::new(), true);
        let lines = take(&mut cable);
        assert_eq!(lines[0], "PPT/1 4 500 text/plain;charset=utf-8");
    }
}
