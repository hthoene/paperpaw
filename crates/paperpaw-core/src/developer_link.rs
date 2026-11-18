//! The developer link over USB (#12, `designs/usb-dev-tools.md`): a
//! computer on the cable may press keys, take screenshots and read and
//! write books and plugin files, once the reader has allowed it on the
//! device. The kernel interprets every line itself (`DEV/1 <n> <command>`,
//! beside the cable transfer's `PPT/1` and the log, `docs/developer-link.md`);
//! no plugin sees the bytes and nothing but the kernel's own commands
//! exists, so the rules hold whoever is on the cable.
//!
//! The rules, each enforced here or in [`Kernel::developer_line`]
//! (`developer_link_run.rs`) and tested in `tests/developer_link.rs`:
//!
//! 1. **Off by default; on only after a question on the device**
//!    (*Customise > Developer*, [`SETTING`]). The channel is off while the
//!    question is shown, so no key sent over it can answer it; the answer
//!    must carry the question's random nonce ([`Link::asked`]).
//! 2. **Off after every restart**: the state is RAM only. The setting is
//!    never written to the card, and what a file says of it is ignored.
//! 3. **A mark while on** (`DEV`, drawn over every screen).
//! 4. **Off at any time**: the row, 10 minutes without a command, the
//!    cable pulled, sleep, ten refusals in a minute.
//! 5. **No secrets**: [`path`] refuses `.paperpaw`, Wi-Fi and secret files,
//!    keys and everything outside the allowed folders.
//! 6. **Allowed folders** only: the library folder, `plugins/<id>/` (no
//!    code, no code plugin's folder), `screenshots/` to read.
//! 7. **No code in**: no `.ppn`, `.so`, firmware or `plugin-code.txt`; no
//!    command installs, updates, flashes or sets a setting; no `key` while a
//!    question or a lock screen is shown.
//! 8. **Rate limits** on commands, file sizes and silence.

use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_platform::buttons;

/// What a line of the developer link begins with.
pub const PREFIX: &str = "DEV/1 ";
/// The setting that opens the question; its value only mirrors the state.
pub(crate) const SETTING: &str = "core.usb-link";
/// Without a command for this long, the link ends.
pub(crate) const IDLE_MS: u64 = 600_000;
/// Commands in one second at most.
const PER_SECOND: u8 = 20;
/// Refusals in [`REFUSAL_WINDOW_MS`] that end the link.
const REFUSALS: u8 = 10;
const REFUSAL_WINDOW_MS: u64 = 60_000;
/// A put is at most this long, a get too.
pub(crate) const MAX_PUT: u64 = 4 << 20;
pub(crate) const MAX_GET: u64 = 16 << 20;
/// A put with no data for this long is given up.
pub(crate) const QUIET_MS: u64 = 5_000;
/// Bytes of a file in one `data` line (as the cable's).
pub(crate) const DATA: usize = 512;
/// Files the link wrote that it may remove again, at most.
const WROTE: usize = 32;
/// The word after `DEV/1 <n> err` for a refused command.
pub(crate) const DENIED: &str = "denied";

/// The keys a computer may press (no power key).
const KEYS: [&str; 10] = [
    buttons::PAGE_FORWARD,
    buttons::PAGE_BACK,
    buttons::UP,
    buttons::DOWN,
    buttons::LEFT,
    buttons::RIGHT,
    buttons::CONFIRM,
    buttons::BACK,
    buttons::MENU,
    buttons::HOME,
];

/// A command of the computer's.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command<'a> {
    Hello,
    Ping,
    Close,
    Screen,
    Shot,
    Describe,
    Heap,
    Key(&'static str),
    Run(&'a str),
    Ls(&'a str),
    Get(&'a str),
    Md5(&'a str),
    Rm(&'a str),
    Put { size: u64, path: &'a str },
    Data(&'a str),
}

/// The line read: its number and the command, or why it is no command.
pub(crate) fn parse(line: &str) -> Option<(u32, Result<Command<'_>, &'static str>)> {
    let rest = line.trim_end().strip_prefix(PREFIX)?;
    let (n, rest) = rest.trim_start().split_once(' ').unwrap_or((rest, ""));
    let n = n.trim().parse::<u32>().ok()?;
    let rest = rest.trim();
    let (verb, args) = rest.split_once(' ').unwrap_or((rest, ""));
    let args = args.trim();
    let command = match verb {
        "hello" => Ok(Command::Hello),
        "ping" => Ok(Command::Ping),
        "close" => Ok(Command::Close),
        "screen" => Ok(Command::Screen),
        "shot" => Ok(Command::Shot),
        "describe" => Ok(Command::Describe),
        "heap" => Ok(Command::Heap),
        "key" => KEYS
            .iter()
            .find(|key| **key == args)
            .map(|key| Command::Key(key))
            .ok_or("not a key"),
        "run" if !args.is_empty() => Ok(Command::Run(args)),
        "ls" => Ok(Command::Ls(args)),
        "get" if !args.is_empty() => Ok(Command::Get(args)),
        "md5" if !args.is_empty() => Ok(Command::Md5(args)),
        "rm" if !args.is_empty() => Ok(Command::Rm(args)),
        "put" => match args.split_once(' ') {
            Some((size, path)) => size
                .parse()
                .map(|size| Command::Put {
                    size,
                    path: path.trim(),
                })
                .map_err(|_| "bad size"),
            None => Err("put <size> <path>"),
        },
        "data" => Ok(Command::Data(args)),
        _ => Err("no such command"),
    };
    Some((n, command))
}

/// What a path is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Use {
    /// Listed, read, hashed.
    Read,
    /// Written (a new file).
    Write,
    /// Removed.
    Remove,
}

/// Where a path is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Root {
    /// The library folder (or what is below it).
    Library,
    /// `plugins/<id>/…` (`plugins` itself to list).
    Plugins,
    /// `screenshots/…`, read only.
    Screenshots,
    /// The roots themselves, listed (no path).
    Top,
}

/// A path the computer named, made safe: as the storage's (no leading
/// `/`), in the spelling of the folders, and which folder it is in; or
/// refused. One function for every file command (rules 5 to 7).
///
/// Refused: a `.`/`..`/empty part (`a//b`), a part ending in a dot or a
/// space or with a `~` (FAT's short names), a part named `.paperpaw`,
/// `wifi.ini`, `secrets…`, `*.key`, `*.sec`, `*.pem` (any case), a path
/// outside the folders, one under `plugins` or `screenshots` that spells
/// a part with other than ASCII (a lookalike of a letter folds to it on
/// FAT), a write below `screenshots`, one to `plugins` itself, and a
/// write of code.
pub(crate) fn path(raw: &str, library: &str, usage: Use) -> Result<(Root, String), &'static str> {
    if raw.is_empty() || raw.len() > 240 || raw.chars().any(forbidden_char) {
        return Err(DENIED);
    }
    let raw = raw.strip_prefix('/').unwrap_or(raw);
    if raw.is_empty() {
        return if usage == Use::Read {
            Ok((Root::Top, String::new()))
        } else {
            Err(DENIED)
        };
    }
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.iter().any(|part| !plain(part)) {
        return Err(DENIED);
    }
    let lib: Vec<&str> = library.split('/').filter(|p| !p.is_empty()).collect();
    let in_library = !lib.is_empty()
        && lib.iter().all(|part| plain(part) && !secret(part))
        && parts.len() >= lib.len()
        && parts
            .iter()
            .zip(&lib)
            .all(|(a, b)| a.eq_ignore_ascii_case(b));
    let (root, below) = if in_library {
        (Root::Library, &parts[lib.len()..])
    } else if parts[0].eq_ignore_ascii_case("plugins") {
        (Root::Plugins, &parts[1..])
    } else if parts[0].eq_ignore_ascii_case("screenshots") {
        (Root::Screenshots, &parts[1..])
    } else {
        return Err(DENIED);
    };
    // Letters that fold to ASCII ones are not told apart by a spelling
    // check: only the library, whose files hold no secrets, has names
    // beyond ASCII.
    if root != Root::Library && below.iter().any(|part| !part.is_ascii()) {
        return Err(DENIED);
    }
    if below.iter().any(|part| secret(part)) {
        return Err(DENIED);
    }
    match (root, usage) {
        (Root::Screenshots, Use::Write | Use::Remove) => return Err(DENIED),
        (Root::Library, Use::Remove) => return Err(DENIED),
        (Root::Plugins, Use::Write | Use::Remove) if below.is_empty() => return Err(DENIED),
        _ => {}
    }
    if usage == Use::Write && below.last().is_some_and(|name| is_code(name)) {
        return Err(DENIED);
    }
    // The spelling of the folders, the rest as the computer wrote it.
    let mut out = String::new();
    let head: &[&str] = match root {
        Root::Library => &lib,
        Root::Plugins => &["plugins"],
        _ => &["screenshots"],
    };
    for part in head.iter().chain(below) {
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    Ok((root, out))
}

/// The `<id>` of a path below `plugins`, if it has one.
pub(crate) fn plugin_folder(path: &str) -> Option<&str> {
    path.strip_prefix("plugins/")?.split('/').next()
}

fn forbidden_char(c: char) -> bool {
    c.is_control()
        || matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
        || matches!(
            c,
            '\u{2028}' | '\u{2029}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}'
                | '\u{2066}'..='\u{2069}' | '\u{061C}'
        )
}

/// A part of a path with nothing a file system reads otherwise.
fn plain(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && !part.ends_with(['.', ' '])
        && !part.starts_with(' ')
        && !part.contains('~')
}

/// A name of what is never sent or listed (rule 5).
pub(crate) fn secret(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == ".paperpaw"
        || name == "wifi.ini"
        || name.starts_with("secret")
        || [".key", ".sec", ".pem"].iter().any(|e| name.ends_with(e))
}

/// A name of code or a firmware image: never written (rule 7).
fn is_code(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name == "plugin-code.txt"
        || [".ppn", ".so", ".bin", ".elf", ".ppu"]
            .iter()
            .any(|e| name.ends_with(e))
}

/// The state of the link (RAM only: a new kernel is off).
#[derive(Default)]
pub(crate) struct Link {
    /// The link is on: the reader allowed it.
    pub(crate) on: bool,
    /// The question's nonce while it is shown.
    pub(crate) asked: Option<u128>,
    /// When the last command came.
    last: u64,
    /// The second the commands are counted in, and how many.
    second: (u64, u8),
    /// The minute the refusals are counted in, and how many.
    minute: (u64, u8),
    /// When the last `err off` was said (one a second at most).
    pub(crate) said_off: Option<u64>,
    /// The file being put.
    pub(crate) put: Option<Put>,
    /// What the link wrote, which it may remove again.
    pub(crate) wrote: Vec<String>,
    /// Why the link ended, until the shell told the computer.
    pub(crate) notice: Option<&'static str>,
}

/// A file coming (`put`): written beside its target, moved over it when whole.
pub(crate) struct Put {
    pub(crate) n: u32,
    pub(crate) path: String,
    pub(crate) temp: String,
    pub(crate) root: Root,
    pub(crate) size: u64,
    pub(crate) got: u64,
    pub(crate) last: u64,
}

impl Link {
    /// Switches on: the reader said yes.
    pub(crate) fn switch_on(&mut self, now: u64) {
        *self = Self {
            on: true,
            last: now,
            ..Self::default()
        };
    }

    /// Ends the link, saying why (the computer is told if it is there).
    /// What was written stays; a put under way is the caller's to drop.
    pub(crate) fn switch_off(&mut self, why: &'static str) {
        if self.on {
            self.notice = Some(why);
        }
        let put = self.put.take();
        *self = Self {
            notice: self.notice,
            put,
            ..Self::default()
        };
    }

    /// Whether a command is let through now (rule 8): not over
    /// [`PER_SECOND`] in a second.
    pub(crate) fn admit(&mut self, now: u64) -> bool {
        self.last = now;
        if now.saturating_sub(self.second.0) >= 1000 {
            self.second = (now, 0);
        }
        self.second.1 = self.second.1.saturating_add(1);
        self.second.1 <= PER_SECOND
    }

    /// A line of a put's data: no count against the limit (the file's size
    /// is the limit), but it keeps the link from being quiet.
    pub(crate) fn touch(&mut self, now: u64) {
        self.last = now;
    }

    /// A refusal was said; `true` when it was the last the link bears
    /// (ten in a minute: a program that probes).
    pub(crate) fn refused(&mut self, now: u64) -> bool {
        if now.saturating_sub(self.minute.0) >= REFUSAL_WINDOW_MS {
            self.minute = (now, 0);
        }
        self.minute.1 = self.minute.1.saturating_add(1);
        self.minute.1 >= REFUSALS
    }

    /// Whether the link has been quiet too long.
    pub(crate) fn idle(&self, now: u64) -> bool {
        self.on && now.saturating_sub(self.last) >= IDLE_MS
    }

    /// Whether `err off` may be said now (once a second).
    pub(crate) fn may_say_off(&mut self, now: u64) -> bool {
        let ok = self
            .said_off
            .is_none_or(|at| now.saturating_sub(at) >= 1000);
        if ok {
            self.said_off = Some(now);
        }
        ok
    }

    /// Notes a file the link wrote.
    pub(crate) fn wrote(&mut self, path: String) {
        if self.wrote.contains(&path) {
            return;
        }
        if self.wrote.len() == WROTE {
            self.wrote.remove(0);
        }
        self.wrote.push(path);
    }
}
