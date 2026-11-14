//! A fake mail server and socket for the session tests: it speaks as much
//! IMAP and SMTP as the sessions use, in whatever sized pieces the test
//! wants, and plays the socket's part (reads, file reads, TLS switches).

#![allow(
    dead_code,
    missing_debug_implementations,
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests unwrap what they just made"
)]

use std::collections::VecDeque;

use paperpaw_sdk::{CertificateHash, Distrust, KeyHash, MemoryStorage, SocketError, Storage, Tls};

/// What the fake socket tells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// A plain connection is made.
    Connected,
    Secured {
        certificate: CertificateHash,
        key: KeyHash,
        time_checked: bool,
    },
    Untrusted {
        certificate: CertificateHash,
        key: KeyHash,
        why: Distrust,
    },
    Bytes(Vec<u8>),
    /// A file read is complete.
    Saved(String),
    /// The other side closed (or the socket ended).
    Closed,
    Failed(SocketError, String),
}

/// What a test asks of the fake server.
#[derive(Clone)]
pub struct Script {
    /// The server's output arrives in pieces of at most this many bytes.
    pub chunk: usize,
    /// The certificate is not trusted (the hashes come with it).
    pub untrusted: bool,
    /// The clock is not known (TLS dates not checked).
    pub time_checked: bool,
    /// Capabilities of the IMAP server (`IMAP4rev1 AUTH=PLAIN`).
    pub capability: String,
    pub password: String,
    pub user: String,
    /// Messages: (uid, flags, raw message text).
    pub messages: Vec<(u32, String, String)>,
    pub folders: Vec<(String, String)>,
    /// SMTP: extensions after the greeting line, and whether a recipient
    /// is refused.
    pub smtp_extensions: Vec<String>,
    pub refuse_rcpt: Option<String>,
}

impl Default for Script {
    fn default() -> Self {
        Self {
            chunk: 1000,
            untrusted: false,
            time_checked: true,
            capability: "IMAP4rev1 AUTH=PLAIN".into(),
            password: "secret".into(),
            user: "ada".into(),
            messages: Vec::new(),
            folders: vec![
                ("INBOX".into(), "\\HasNoChildren".into()),
                ("Sent".into(), "\\Sent".into()),
            ],
            smtp_extensions: vec![
                "STARTTLS".into(),
                "AUTH PLAIN LOGIN".into(),
                "SIZE 1000000".into(),
            ],
            refuse_rcpt: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Proto {
    Imap,
    Smtp,
}

/// The fake server and the socket in one: implements [`Io`] and collects
/// what the session says.
pub struct Fake {
    pub script: Script,
    pub proto: Proto,
    pub storage: std::rc::Rc<MemoryStorage>,
    /// The server's bytes not yet read.
    out: VecDeque<u8>,
    /// What the client sent, as lines the server has not answered yet.
    inbox: Vec<u8>,
    want_bytes: Option<u32>,
    want_file: Option<(String, u64)>,
    pending: VecDeque<Input>,
    pub secure: bool,
    pub closed: bool,
    pub connected: bool,
    pub sent_plain_password: bool,
    pub sent: Vec<String>,
    pub tls_switches: u32,
    pub tls_used: Vec<Tls>,
    selected: Option<String>,
    flags: Vec<(u32, String)>,
    in_data: Option<Vec<u8>>,
    auth: Auth,
    pending_tag: String,
    pub smtp_got: Option<(String, Vec<String>, String)>,
    smtp_from: String,
    smtp_to: Vec<String>,
    pub starttls_pending: bool,
    pub max_reads: u32,
    pub reads: u32,
    /// After a STARTTLS go-ahead the server sends nothing until switched.
    switching: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Auth {
    None,
    PlainWait,
    LoginUser,
    LoginPass,
}

impl Fake {
    pub fn new(proto: Proto, script: Script) -> Self {
        Self {
            script,
            proto,
            storage: std::rc::Rc::new(MemoryStorage::new()),
            out: VecDeque::new(),
            inbox: Vec::new(),
            want_bytes: None,
            want_file: None,
            pending: VecDeque::new(),
            secure: false,
            closed: false,
            connected: false,
            sent_plain_password: false,
            sent: Vec::new(),
            tls_switches: 0,
            tls_used: Vec::new(),
            selected: None,
            flags: Vec::new(),
            in_data: None,
            auth: Auth::None,
            pending_tag: String::new(),
            smtp_got: None,
            smtp_from: String::new(),
            smtp_to: Vec::new(),
            starttls_pending: false,
            max_reads: 10_000,
            reads: 0,
            switching: false,
        }
    }

    fn say(&mut self, text: &str) {
        self.out.extend(text.bytes());
    }

    fn greet(&mut self) {
        match self.proto {
            Proto::Imap => {
                let caps = self.script.capability.clone();
                self.say(&format!("* OK [CAPABILITY {caps}] ready\r\n"));
            }
            Proto::Smtp => self.say("220 mail.example.org ESMTP\r\n"),
        }
    }

    /// The input the session gets next, if the fake has one; runs the
    /// socket.
    pub fn next_input(&mut self) -> Option<Input> {
        if let Some(i) = self.pending.pop_front() {
            return Some(i);
        }
        if self.switching {
            return None;
        }
        if let Some((path, len)) = self.want_file.clone() {
            if self.out.len() as u64 >= len {
                let data: Vec<u8> = self.out.drain(..len as usize).collect();
                self.storage.write(&path, &data).unwrap();
                self.want_file = None;
                return Some(Input::Saved(path));
            }
            return None;
        }
        if let Some(max) = self.want_bytes {
            if !self.out.is_empty() {
                let n = (max as usize).min(self.script.chunk).min(self.out.len());
                let data: Vec<u8> = self.out.drain(..n).collect();
                self.want_bytes = None;
                self.reads += 1;
                assert!(self.reads < self.max_reads, "too many reads");
                return Some(Input::Bytes(data));
            }
            if self.closed {
                self.want_bytes = None;
                return Some(Input::Bytes(Vec::new()));
            }
        }
        None
    }

    fn input_from_client(&mut self, data: &[u8]) {
        assert!(!self.switching, "sent while the TLS switch is under way");
        if !self.secure && self.proto == Proto::Imap {
            // Anything with the password before TLS is a leak.
            let text = String::from_utf8_lossy(data);
            if text.contains(&self.script.password)
                || text.contains("AUTHENTICATE")
                || text.contains("LOGIN")
            {
                self.sent_plain_password = true;
            }
        }
        if !self.secure && self.proto == Proto::Smtp {
            let text = String::from_utf8_lossy(data);
            if text.starts_with("AUTH") {
                self.sent_plain_password = true;
            }
        }
        if let Some(buf) = &mut self.in_data {
            buf.extend_from_slice(data);
            if buf.ends_with(b"\r\n.\r\n") {
                let body = String::from_utf8_lossy(&buf[..buf.len() - 3]).into_owned();
                self.in_data = None;
                self.smtp_got = Some((self.smtp_from.clone(), self.smtp_to.clone(), body));
                self.say("250 2.0.0 queued\r\n");
            }
            return;
        }
        self.inbox.extend_from_slice(data);
        while let Some(end) = self.inbox.windows(2).position(|w| w == b"\r\n") {
            let line: Vec<u8> = self.inbox.drain(..end + 2).collect();
            let line = String::from_utf8_lossy(&line[..line.len() - 2]).into_owned();
            self.sent.push(if line.len() > 200 {
                line[..200].into()
            } else {
                line.clone()
            });
            match self.proto {
                Proto::Imap => self.imap_line(&line),
                Proto::Smtp => self.smtp_line(&line),
            }
            if self.in_data.is_some() && !self.inbox.is_empty() {
                let rest = std::mem::take(&mut self.inbox);
                self.input_from_client(&rest);
            }
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "the fake server's whole IMAP dialogue in one match"
    )]
    fn imap_line(&mut self, line: &str) {
        if self.auth == Auth::PlainWait {
            self.auth = Auth::None;
            let tag = self.pending_tag.clone();
            let expect = paperpaw_plugin_mail::native_model::codec::base64_line(
                format!("\0{}\0{}", self.script.user, self.script.password).as_bytes(),
            )
            .unwrap();
            if line == expect {
                self.say(&format!("{tag} OK authenticated\r\n"));
            } else {
                self.say(&format!(
                    "{tag} NO [AUTHENTICATIONFAILED] bad credentials\r\n"
                ));
            }
            return;
        }
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        let tag = tag.to_string();
        let upper = rest.to_ascii_uppercase();
        if upper == "CAPABILITY" {
            let caps = self.script.capability.clone();
            self.say(&format!("* CAPABILITY {caps}\r\n{tag} OK done\r\n"));
        } else if upper == "STARTTLS" {
            self.say(&format!("{tag} OK begin TLS\r\n"));
            self.starttls_pending = true;
        } else if upper.starts_with("AUTHENTICATE PLAIN") {
            let arg = rest["AUTHENTICATE PLAIN".len()..].trim();
            if arg.is_empty() {
                self.auth = Auth::PlainWait;
                self.pending_tag = tag;
                self.say("+ \r\n");
            } else {
                let expect = paperpaw_plugin_mail::native_model::codec::base64_line(
                    format!("\0{}\0{}", self.script.user, self.script.password).as_bytes(),
                )
                .unwrap_or_default();
                if arg == expect {
                    self.say(&format!("{tag} OK authenticated\r\n"));
                } else {
                    self.say(&format!(
                        "{tag} NO [AUTHENTICATIONFAILED] bad credentials\r\n"
                    ));
                }
            }
        } else if upper.starts_with("LOGIN ") {
            let ok = rest.contains(&format!("\"{}\"", self.script.password));
            self.say(&if ok {
                format!("{tag} OK logged in\r\n")
            } else {
                format!("{tag} NO login failed\r\n")
            });
        } else if upper.starts_with("LIST") {
            let mut text = String::new();
            for (name, flags) in &self.script.folders {
                text.push_str(&format!("* LIST ({flags}) \"/\" \"{name}\"\r\n"));
            }
            text.push_str(&format!("{tag} OK done\r\n"));
            self.say(&text);
        } else if upper.starts_with("SELECT") {
            let name = rest[6..].trim().trim_matches('"').to_string();
            if self.script.folders.iter().any(|(n, _)| *n == name) {
                self.selected = Some(name);
                let n = self.script.messages.len();
                self.say(&format!(
                    "* {n} EXISTS\r\n* OK [UIDVALIDITY 77] ok\r\n{tag} OK [READ-WRITE] done\r\n"
                ));
            } else {
                self.say(&format!("{tag} NO no such folder\r\n"));
            }
        } else if upper.starts_with("STATUS") {
            let unseen = self
                .script
                .messages
                .iter()
                .filter(|(_, f, _)| !f.contains("\\Seen"))
                .count();
            let n = self.script.messages.len();
            self.say(&format!(
                "* STATUS \"INBOX\" (MESSAGES {n} UNSEEN {unseen})\r\n{tag} OK done\r\n"
            ));
        } else if upper.starts_with("FETCH ") {
            let range = rest[6..].split(' ').next().unwrap().to_string();
            let (a, b) = range.split_once(':').unwrap();
            let (a, b): (usize, usize) = (a.parse().unwrap(), b.parse().unwrap());
            let mut text = String::new();
            for (i, (uid, flags, raw)) in self.script.messages.iter().enumerate() {
                let seq = i + 1;
                if seq < a || seq > b {
                    continue;
                }
                let head = raw.split("\r\n\r\n").next().unwrap_or("");
                let head = format!("{head}\r\n\r\n");
                text.push_str(&format!(
                    "* {seq} FETCH (UID {uid} FLAGS ({flags}) INTERNALDATE \"03-Nov-2026 09:15:00 +0100\" RFC822.SIZE {} BODY[HEADER.FIELDS (FROM TO CC SUBJECT DATE MESSAGE-ID)] {{{}}}\r\n{head})\r\n",
                    raw.len(),
                    head.len()
                ));
            }
            text.push_str(&format!("{tag} OK done\r\n"));
            self.say(&text);
        } else if upper.starts_with("UID FETCH") {
            let uid: u32 = rest.split(' ').nth(2).unwrap().parse().unwrap();
            let found = self
                .script
                .messages
                .iter()
                .find(|(u, _, _)| *u == uid)
                .cloned();
            match found {
                Some((_, _, raw)) => {
                    let limit: usize = rest
                        .split("0.")
                        .nth(1)
                        .and_then(|s| s.trim_end_matches('>').parse().ok())
                        .unwrap_or(usize::MAX);
                    let bytes = &raw.as_bytes()[..raw.len().min(limit)];
                    self.say(&format!(
                        "* 1 FETCH (UID {uid} BODY[]<0> {{{}}}\r\n",
                        bytes.len()
                    ));
                    self.out.extend(bytes.iter().copied());
                    self.say(&format!(")\r\n{tag} OK done\r\n"));
                }
                None => self.say(&format!("{tag} OK nothing\r\n")),
            }
        } else if upper.starts_with("UID STORE") {
            let words: Vec<&str> = rest.split(' ').collect();
            let uid: u32 = words[2].parse().unwrap();
            self.flags.push((uid, rest.to_string()));
            self.say(&format!("{tag} OK stored\r\n"));
        } else if upper == "LOGOUT" {
            self.say(&format!("* BYE bye\r\n{tag} OK logged out\r\n"));
            self.closed = true;
        } else {
            self.say(&format!("{tag} BAD unknown\r\n"));
        }
    }

    fn smtp_line(&mut self, line: &str) {
        let upper = line.to_ascii_uppercase();
        match self.auth {
            Auth::LoginUser => {
                self.auth = Auth::LoginPass;
                self.say("334 UGFzc3dvcmQ6\r\n");
                return;
            }
            Auth::LoginPass => {
                self.auth = Auth::None;
                let pw = paperpaw_plugin_mail::native_model::codec::base64_line(
                    self.script.password.as_bytes(),
                )
                .unwrap();
                self.say(if line == pw {
                    "235 ok\r\n"
                } else {
                    "535 bad\r\n"
                });
                return;
            }
            _ => {}
        }
        if upper.starts_with("EHLO") {
            let mut text = String::from("250-mail.example.org\r\n");
            let exts: Vec<String> = self
                .script
                .smtp_extensions
                .iter()
                .filter(|e| self.secure || !e.starts_with("AUTH") || true)
                .cloned()
                .collect();
            for (i, e) in exts.iter().enumerate() {
                let sep = if i + 1 == exts.len() { ' ' } else { '-' };
                // STARTTLS is not offered again once secure.
                if e == "STARTTLS" && self.secure {
                    continue;
                }
                text.push_str(&format!("250{sep}{e}\r\n"));
            }
            self.say(&text);
        } else if upper == "STARTTLS" {
            self.say("220 go ahead\r\n");
            self.starttls_pending = true;
        } else if upper.starts_with("AUTH PLAIN ") {
            let expect = paperpaw_plugin_mail::native_model::codec::base64_line(
                format!("\0{}\0{}", self.script.user, self.script.password).as_bytes(),
            )
            .unwrap();
            self.say(if line[11..] == expect {
                "235 ok\r\n"
            } else {
                "535 5.7.8 bad credentials\r\n"
            });
        } else if upper == "AUTH LOGIN" {
            self.auth = Auth::LoginUser;
            self.say("334 VXNlcm5hbWU6\r\n");
        } else if upper.starts_with("MAIL FROM:") {
            self.smtp_from = line[10..].trim_matches(['<', '>']).to_string();
            self.say("250 ok\r\n");
        } else if upper.starts_with("RCPT TO:") {
            let to = line[8..].trim_matches(['<', '>']).to_string();
            if self.script.refuse_rcpt.as_deref() == Some(&to) {
                self.say("550 5.1.1 no such user\r\n");
            } else {
                self.smtp_to.push(to);
                self.say("250 ok\r\n");
            }
        } else if upper == "DATA" {
            self.in_data = Some(Vec::new());
            self.say("354 go\r\n");
        } else if upper == "QUIT" {
            self.say("221 bye\r\n");
            self.closed = true;
        } else {
            self.say("500 what\r\n");
        }
    }
}

impl Fake {
    pub fn connect_tls(&mut self, _host: &str, _port: u16, tls: Tls) {
        self.tls_used.push(tls);
        self.connected = true;
        self.secure = true;
        self.greet();
        self.pending.push_back(self.secured());
    }

    pub fn connect_plain(&mut self, _host: &str, _port: u16) {
        self.connected = true;
        self.greet();
        self.pending.push_back(Input::Connected);
    }

    pub fn start_tls(&mut self, tls: Tls) {
        assert!(
            self.starttls_pending,
            "start_tls without the server's go-ahead"
        );
        self.starttls_pending = false;
        self.tls_used.push(tls);
        self.tls_switches += 1;
        self.switching = true;
        self.out.clear();
        self.want_bytes = None;
        self.secure = true;
        let secured = self.secured();
        self.pending.push_back(secured);
        // The first thing the client may do after is read.
    }

    pub fn send(&mut self, data: Vec<u8>) {
        self.input_from_client(&data);
    }

    pub fn read_bytes(&mut self, max: u32) {
        // The real kernel answers a read while the socket is secured with
        // `NotReady` and closes the socket.
        assert!(!self.switching, "read while the TLS switch is under way");
        assert!(
            self.want_bytes.is_none() && self.want_file.is_none(),
            "two reads at once"
        );
        self.want_bytes = Some(max);
    }

    pub fn read_file(&mut self, path: &str, len: u64) {
        assert!(!self.switching, "read while the TLS switch is under way");
        assert!(
            self.want_bytes.is_none() && self.want_file.is_none(),
            "two reads at once"
        );
        self.want_file = Some((path.to_string(), len));
    }

    pub fn close(&mut self) {
        self.closed = true;
    }
}

impl Fake {
    fn secured(&self) -> Input {
        if self.script.untrusted {
            return Input::Untrusted {
                certificate: CertificateHash::new([7; 32]),
                key: KeyHash::new([9; 32]),
                why: paperpaw_sdk::Distrust::UnknownIssuer,
            };
        }
        Input::Secured {
            certificate: CertificateHash::new([7; 32]),
            key: KeyHash::new([9; 32]),
            time_checked: self.script.time_checked,
        }
    }

    /// Called by the harness once `Secured` is delivered after a switch:
    /// the server may speak again.
    pub fn switched(&mut self) {
        self.switching = false;
    }
}
