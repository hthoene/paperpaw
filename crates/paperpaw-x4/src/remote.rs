//! The X4's remote control over USB, for testing on the device: a computer
//! sends lines with commands (`tools/x4-remote.py` does), the X4 answers
//! with lines that start with `PP:`, amid its log. Here what is the same
//! on any X4 firmware: the commands read from lines ([`parse`]) and how
//! data is written in answers; the firmware carries them out.
//!
//! - `press <button>`: the button pressed and let go;
//! - `open <path>`: opens the book at `path` (hexadecimal) in the reader
//!   (`PP:OK`);
//! - `run <action>`: runs an action as a button bound to it would, e.g.
//!   `run calibre.open` to open a tool at once (`PP:OK`);
//! - `slot <slot> [<key>=<hex value> …]`: opens a slot (`core.settings`,
//!   `core.home` …) with parameters, as a plugin would (`PP:OK`);
//! - `show <screen> [<key>=<hex value> …]`: opens a screen by its id with
//!   parameters (`PP:OK`);
//! - `set <setting> <value>`: sets a setting as the settings screen would,
//!   the value as configuration files write it, in hexadecimal (`PP:OK`,
//!   or `PP:ERR` and why);
//! - `setting <setting>`: `PP:SETTING <value>`, as configuration files
//!   write it;
//! - `hold <button> <ms>`: held that long, e.g. for a long press;
//! - `shot`: the screen, as `PP:FRAME <width> <height> <bits per pixel>`,
//!   then `PP:ROW <y> <hex>` for every row (its bytes as the frame holds
//!   them, in [`runs`]) and `PP:END`;
//! - `screen`: `PP:SCREEN <id>`, the screen shown;
//! - `describe`: what it shows, a `PP:PART <role> <x> <y> <width> <height>
//!   <state> <label>` for each row, button or heading (state: see
//!   [`part_state`]), then `PP:END`;
//! - `heap`: `PP:HEAP <used> <free> <stack left> <stack size> <largest>
//!   <peak>`, in bytes (the stack: what it never used since start; the
//!   largest block that could be had now, of the free memory in pieces;
//!   the most the heap ever held since start);
//! - `input`: `PP:INPUT <readings> <front> <side> <power> <millivolts>`, how
//!   the buttons and the battery are read;
//! - `profile start`, `profile stop`: notes where the processor is every
//!   5 ms in between, sent as `PP:PC <hex> …` lines and `PP:END`;
//! - `trace start`, `trace stop`: what is done on the storage in between,
//!   as `PP:IO <what> <path> …` lines and `PP:END`;
//! - `peek <address> <words>`: memory, as `PP:MEM <address> <hex words>`
//!   lines of 16 words and `PP:END` (hexadecimal address, at most
//!   [`PEEK_WORDS`] words; only the [`ROM`], e.g. to find where its
//!   functions lie);
//! - `refresh fast`, `refresh full`: the panel refreshed from what the
//!   controller holds, nothing sent (`PP:OK`): whether it holds the frame;
//! - `gray [<path>]`: four grays on the panel to judge them, with the
//!   decoded gray picture at `path` (hexadecimal) below; then the screen
//!   is drawn again (`PP:OK`);
//! - `put <size> <path>`: a file for the storage (the SD card), answered
//!   with `PP:READY`; then lines `data <hex>` until all its bytes came, and
//!   `PP:OK <size>` once it is written;
//! - `idle sleep <seconds>`: light sleep as on battery for that long
//!   (`PP:OK`; the connection goes meanwhile), then start again;
//! - `hang <seconds>`: hangs that long (at most a minute) without the
//!   watchdog fed, to try that it starts the X4 again (`PP:OK` if not);
//! - `boot`: `PP:BOOT <reason> <place> <naps>`, why the X4 last started,
//!   where the run before was then and its light sleeps, e.g. after a
//!   watchdog's reset or an `idle sleep`, and `PP:PANIC <message>` if the
//!   run before panicked. `PP:NAPS <count>` reports completed light sleeps
//!   in the current run, so a battery test can verify that it really slept;
//! - `graytone <stock|middle>`: how dark the X4's grays between
//!   come out from now on, to compare them (`PP:OK`);
//! - `nap <seconds>`: one light sleep that long (at most a minute), then a
//!   new start, to try that the RTC's watchdog starts it again first;
//! - `wifi scan`: switches the radio on and lists the networks in reach,
//!   as `PP:NET <signal> <secured> <name>` lines, then `PP:END`;
//! - `wifi connect <name> <password>`: joins that network (both in
//!   hexadecimal, `-` for no password), answered once joined with
//!   `PP:CONNECTED <name>` and `PP:IP <address>`, or `PP:ERR`; the
//!   password is not shown anywhere and is kept with the passwords and
//!   keys, where the `wifi` plugin finds it to join the network again;
//! - `wifi status`: `PP:WIFI <off|on|joining|online> [<name> <address>]`;
//! - `wifi off`: switches the radio off (`PP:OK`);
//! - `http get <url> [<path>]`: fetches a plain `http://` address as
//!   plugins' requests are made, into memory or the file `path`, answered
//!   with `PP:HTTP <status> <bytes> saved <true|false>`, the heap and, for
//!   a failure, why (the radio must be online);
//! - `get <path>`: a file of the storage, as `PP:SIZE <size>`, then
//!   `PP:DATA <hex>` lines and `PP:END` (e.g. the battery's log).
//!
//! The storage (paths are the rest of the line, spaces and all; the card's
//! top is an empty path):
//!
//! - `ls [<path>]`: a folder, a `PP:ENTRY <d|f> <size> <name>` line for
//!   each entry, then `PP:END`;
//! - `stat <path>`: `PP:STAT <d|f|-> <size>` (`-`: not there);
//! - `md5 <path>`: `PP:MD5 <hex> <size>`, to check a file that came;
//! - `rm <path>`: removes a file, or a folder with all in it (`PP:OK`);
//! - `mkdir <path>`: makes a folder and those above it (`PP:OK`);
//! - `mv <from> <to>`: renames or moves (both in hexadecimal, `PP:OK`);
//! - `sync`: what the storage kept back written to the card (`PP:OK`).
//!
//! What runs:
//!
//! - `version`: `PP:VERSION <version> <device>`, then a `PP:CRATE <name>
//!   <version>` line for each plugin crate built in and `PP:END`;
//! - `plugins`: a `PP:PLUGIN <id> <on|off> <version>` line for each plugin,
//!   then `PP:END`;
//! - `actions [<prefix>]`: a `PP:ACTION <id>` line for each action (those
//!   whose ids begin with `prefix`), then `PP:END`;
//! - `settings [<prefix>]`: a `PP:VALUE <id> <value>` line for each
//!   setting (as configuration files write it), then `PP:END`;
//! - `screens`: `PP:SCREENS <id> …`, the screens open, the one shown last;
//! - `usage`: `PP:USAGE <fast> <partial> <full> <pixels> <inputs> <sleeps>
//!   <panel ms>` since start, then a `PP:USE <plugin> <busy µs> <calls>
//!   <shown ms> <fast> <partial> <full> <bytes read> <bytes written>` line
//!   for each plugin and `PP:END` (as of the last input or tick);
//! - `time`: `PP:TIME <unix ms or -> <ms since start>`;
//! - `log <off|error|warn|info|debug|trace>`: how much the log says
//!   (`PP:OK`).
//!
//! Changes:
//!
//! - `emit <event> [<key>=<hex value> …]`: an event, as a plugin would
//!   send it (`PP:OK`);
//! - `type <text>`, `type-enter <text>`: the text (hexadecimal, `-` for
//!   none) typed into the text field shown (the on-screen keyboard's, or
//!   whatever screen is bound to `core.keyboard`) as if on its keys,
//!   plugins seeing it as typed; with `type-enter` it is then confirmed
//!   (`PP:OK`, or `PP:ERR no text field` where none shows);
//! - `touch <down|move|up> <x> <y>`: a finger on the screen, where
//!   PaperPaw draws (`PP:OK`), e.g. to tap or swipe on the X4 Pro;
//! - `drag <x1> <y1> <x2> <y2> <steps> <ms>`: a finger put down at the
//!   first point, moved to the second in `steps` steps `ms` apart and
//!   lifted, read as the touch controller's readings are (in their
//!   interrupt, also while the main loop is busy); `PP:OK` at once;
//! - `clicks <button> <count> <every ms> <held ms> [<jitter ms>]`: the
//!   button clicked `count` times, read as its own readings are (in their
//!   interrupt, also while the main loop is busy), to try how presses get
//!   through while PaperPaw works; with a jitter the gaps vary by up to so
//!   much either way, as a person's do ([`click_gap`]); `PP:OK` at once;
//! - `time set <unix ms>`: the clock set (`PP:OK`);
//! - `restart`: what plugins keep saved, then the X4 starts again
//!   (`PP:OK` first; the connection comes back by itself);
//! - `panic <message>`: panics with that message (hexadecimal), to try
//!   what a crash leaves (`boot` says it after the restart).
//!
//! Any other line is answered with `PP:ERR <line>`.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use core::ops::Range;

use crate::buttons;

/// How long `press` holds a button, in milliseconds.
pub const PRESS_MS: u64 = 60;

/// The button a `press` or a `hold` keeps down, and when it is let go.
///
/// The time counts from the press itself, not from the end of what the
/// press brought: where a frame waited to be drawn, the press shows it,
/// and the panel takes half a second over that, as long as a long press
/// takes (`core.long-press-ms`). Counted from the frame's end the release
/// came after the kernel's next tick, which took the 60 ms click for a
/// long press (the first Right on Home opened the quick panel).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Hold(Option<(&'static str, u64)>);

impl Hold {
    /// `button` went down at `pressed` and stays down for `ms`.
    pub fn press(&mut self, button: &'static str, pressed: u64, ms: u64) {
        self.0 = Some((button, pressed.saturating_add(ms)));
    }

    /// Whether a button is down.
    pub fn is_down(&self) -> bool {
        self.0.is_some()
    }

    /// Whether the button down is to be let go at `now`: asked before the
    /// kernel is given a later time, so that it never sees the button
    /// held longer than asked.
    pub fn due(&self, now: u64) -> bool {
        self.0.is_some_and(|(_, until)| until <= now)
    }

    /// The button down, let go.
    pub fn take(&mut self) -> Option<&'static str> {
        self.0.take().map(|(button, _)| button)
    }
}

/// The memory `peek` may read: the ROM (elsewhere a read may fault).
pub const ROM: Range<u32> = 0x4000_0000..0x4006_0000;
/// The most words `peek` reads at once.
pub const PEEK_WORDS: u32 = 4096;

/// What the computer asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// A button pressed, let go after so many milliseconds.
    Press {
        button: &'static str,
        ms: u64,
    },
    /// An action run, by its id (checked by the firmware).
    Run(String),
    /// A book opened, by its path.
    Open(String),
    /// A setting set, by its id, to a value as configuration files write
    /// it.
    Set {
        id: String,
        value: String,
    },
    /// A setting's value asked for, by its id.
    Setting(String),
    Shot,
    Screen,
    Describe,
    Heap,
    Input,
    /// Profiling starts (`true`) or stops.
    Profile(bool),
    /// Tracing the storage starts (`true`) or stops.
    Trace(bool),
    /// Words of the ROM from an address.
    Peek {
        at: u32,
        words: u32,
    },
    /// The panel refreshed from the controller's memory, full or fast.
    Refresh {
        full: bool,
    },
    /// Four grays on the panel, with a decoded gray picture if named.
    Gray(Option<String>),
    /// How dark the X4's grays between come out from now on (to compare).
    GrayTone(crate::display::GrayTone),
    /// Light sleep while idle even on USB, for so many seconds.
    IdleSleep(u64),
    /// A file of so many bytes for that path.
    Put {
        size: usize,
        path: String,
    },
    Get(String),
    Boot,
    /// About the network.
    Network(Ask),
    Hang(u64),
    Nap(u64),
    /// A folder's entries.
    List(String),
    /// Whether a path is a file or a folder, and its size.
    Stat(String),
    /// A file's MD5.
    Md5(String),
    /// A file, or a folder with all in it, removed.
    Remove(String),
    MakeDir(String),
    Move {
        from: String,
        to: String,
    },
    Sync,
    Version,
    Plugins,
    /// Where native code plugins lie, and why not.
    Native,
    /// The actions whose ids begin with this.
    Actions(String),
    /// The settings whose ids begin with this, with their values.
    Settings(String),
    Screens,
    Usage,
    Time,
    /// The log's level, as the `log` crate names them.
    Log(&'static str),
    /// An event, with its parameters.
    Emit {
        event: String,
        params: Vec<(String, String)>,
    },
    /// A slot opened, with parameters.
    Slot {
        id: String,
        params: Vec<(String, String)>,
    },
    /// A screen opened by its id, with parameters.
    Show {
        id: String,
        params: Vec<(String, String)>,
    },
    /// Text typed into the text field shown, then confirmed with `done`.
    Type {
        text: String,
        done: bool,
    },
    /// A finger down, moved or up at a point.
    Touch {
        phase: Touch,
        x: i32,
        y: i32,
    },
    /// A finger moved from one point to another as the touch controller
    /// reads it: `steps` moves, `ms` apart.
    Drag {
        from: (u16, u16),
        to: (u16, u16),
        steps: u16,
        ms: u16,
    },
    /// Button `button` (of [`crate::buttons::ALL`]) clicked `count`
    /// times, `every` ms apart (give or take `jitter`, see
    /// [`click_gap`]), held `held` ms each.
    Clicks {
        button: &'static str,
        count: u16,
        every: u16,
        held: u16,
        jitter: u16,
    },
    /// The clock set, to milliseconds since 1970.
    SetTime(i64),
    Restart,
    Panic(String),
}

/// The gap before the next of the remote's clicks: `every` ms, give or
/// take up to `jitter` ms, from the pseudo-random `seed` (a linear
/// congruential generator, which x4-remote reckons the same way, seed 1
/// first, to know when each click began).
pub fn click_gap(every: u16, jitter: u16, seed: &mut u32) -> u16 {
    *seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
    let span = 2 * u32::from(jitter) + 1;
    let step = (*seed >> 16) % span;
    (u32::from(every) - u32::from(jitter) + step) as u16
}

/// What a finger does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Touch {
    Down,
    Move,
    Up,
}

/// The log's levels, from saying nothing to saying everything.
pub const LOG_LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

/// What is asked of the network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ask {
    /// Networks in reach (the radio switched on).
    Scan,
    /// Join a network, kept with its password once joined.
    Connect {
        ssid: String,
        password: String,
    },
    Status,
    Off,
    /// Fetch an address, into memory or the file `save_to`.
    Get {
        url: String,
        save_to: Option<String>,
    },
}

/// The command a line says, if it is one.
pub fn parse(line: &str) -> Option<Command> {
    let line = line.trim();
    let mut words = line.split_ascii_whitespace();
    let name = words.next()?;
    if let Some(command) = whole_line(name, line, &mut words) {
        return command;
    }
    let command = command(name, line, &mut words)?;
    words.next().is_none().then_some(command)
}

/// The command of a line that ends in its last word, `name` its first.
fn command<'a>(
    name: &str,
    line: &str,
    words: &mut impl Iterator<Item = &'a str>,
) -> Option<Command> {
    let command = match name {
        "press" => Command::Press {
            button: button(words.next()?)?,
            ms: PRESS_MS,
        },
        "hold" => Command::Press {
            button: button(words.next()?)?,
            ms: words.next()?.parse().ok()?,
        },
        "shot" => Command::Shot,
        "screen" => Command::Screen,
        "describe" => Command::Describe,
        "heap" => Command::Heap,
        "boot" => Command::Boot,
        "input" => Command::Input,
        "run" => Command::Run(String::from(words.next()?)),
        "open" => Command::Open(unhex(words.next()?)?),
        "set" => Command::Set {
            id: String::from(words.next()?),
            value: unhex(words.next().unwrap_or(""))?,
        },
        "setting" => Command::Setting(String::from(words.next()?)),
        "wifi" => Command::Network(wifi(words)?),
        "http" => match words.next()? {
            "get" => Command::Network(Ask::Get {
                url: String::from(words.next()?),
                save_to: words.next().map(String::from),
            }),
            _ => return None,
        },
        "hang" => Command::Hang(words.next()?.parse().ok()?),
        "sync" => Command::Sync,
        "version" => Command::Version,
        "plugins" => Command::Plugins,
        "native" => Command::Native,
        "screens" => Command::Screens,
        "usage" => Command::Usage,
        "restart" => Command::Restart,
        "actions" => Command::Actions(String::from(words.next().unwrap_or(""))),
        "settings" => Command::Settings(String::from(words.next().unwrap_or(""))),
        "time" => match words.next() {
            None => Command::Time,
            Some("set") => Command::SetTime(words.next()?.parse().ok()?),
            Some(_) => return None,
        },
        "log" => {
            let level = words.next()?;
            Command::Log(LOG_LEVELS.iter().find(|l| **l == level).copied()?)
        }
        "panic" => Command::Panic(unhex(words.next()?)?),
        "mv" => Command::Move {
            from: unhex(words.next()?)?,
            to: unhex(words.next()?)?,
        },
        "drag" => drag(words)?,
        "clicks" => clicks(words)?,
        "touch" => touch(words)?,
        "type" | "type-enter" => Command::Type {
            text: unhex(words.next()?)?,
            done: line.starts_with("type-enter"),
        },
        "emit" => Command::Emit {
            event: String::from(words.next()?),
            params: pairs(words)?,
        },
        "slot" => Command::Slot {
            id: String::from(words.next()?),
            params: pairs(words)?,
        },
        "show" => Command::Show {
            id: String::from(words.next()?),
            params: pairs(words)?,
        },
        "nap" => Command::Nap(words.next()?.parse().ok()?),
        "graytone" => Command::GrayTone(match words.next()? {
            "stock" => crate::display::GrayTone::Stock,
            "middle" => crate::display::GrayTone::Middle,
            _ => return None,
        }),
        "gray" => Command::Gray(match words.next() {
            Some(path) => Some(unhex(path)?),
            None => None,
        }),
        "refresh" => Command::Refresh {
            full: on_off(words.next()?, "full", "fast")?,
        },
        "idle" => match words.next()? {
            "sleep" => Command::IdleSleep(words.next()?.parse().ok()?),
            _ => return None,
        },
        "trace" => Command::Trace(on_off(words.next()?, "start", "stop")?),
        "profile" => Command::Profile(on_off(words.next()?, "start", "stop")?),
        _ => return None,
    };
    Some(command)
}

/// The command of a line that is read to its end, `name` its first word.
/// `None` for any other first word.
fn whole_line<'a>(
    name: &str,
    line: &str,
    words: &mut impl Iterator<Item = &'a str>,
) -> Option<Option<Command>> {
    Some(match name {
        "ls" | "stat" | "md5" | "rm" | "mkdir" => card_path(name, line),
        "peek" => peek(words),
        "get" => get(line),
        "put" => put(line, words),
        _ => return None,
    })
}

/// A command on a card path: the rest of the line.
fn card_path(name: &str, line: &str) -> Option<Command> {
    // Paths on the card are named from its root, with or without `/`.
    let path = line
        .split_once(' ')
        .map_or("", |(_, rest)| rest.trim().trim_matches('/'));
    let path = String::from(path);
    match name {
        "ls" => Some(Command::List(path)),
        _ if path.is_empty() => None,
        "stat" => Some(Command::Stat(path)),
        "md5" => Some(Command::Md5(path)),
        "rm" => Some(Command::Remove(path)),
        _ => Some(Command::MakeDir(path)),
    }
}

/// `peek <hex address> <words>`: words of the ROM.
fn peek<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Command> {
    let at = u32::from_str_radix(words.next()?, 16).ok()?;
    let words: u32 = words.next()?.parse().ok()?;
    let end = at.checked_add(words.checked_mul(4)?)?;
    let fits =
        (1..=PEEK_WORDS).contains(&words) && at % 4 == 0 && ROM.contains(&at) && end <= ROM.end;
    fits.then_some(Command::Peek { at, words })
}

/// `get <path>`: the path is the rest of the line, spaces and all.
fn get(line: &str) -> Option<Command> {
    let path = line.split_once(' ')?.1.trim();
    (!path.is_empty()).then(|| Command::Get(String::from(path)))
}

/// `put <size> <path>`: the path is the rest of the line, spaces and all.
fn put<'a>(line: &str, words: &mut impl Iterator<Item = &'a str>) -> Option<Command> {
    let size = words.next()?.parse().ok()?;
    let path = line
        .split_once(' ')?
        .1
        .trim_start()
        .split_once(' ')?
        .1
        .trim();
    (!path.is_empty()).then(|| Command::Put {
        size,
        path: String::from(path),
    })
}

/// `wifi ...`: what is asked of the network.
fn wifi<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Ask> {
    Some(match words.next()? {
        "scan" => Ask::Scan,
        "connect" => Ask::Connect {
            ssid: unhex(words.next()?)?,
            password: unhex(words.next()?)?,
        },
        "status" => Ask::Status,
        "off" => Ask::Off,
        _ => return None,
    })
}

/// `touch <down|move|up> <x> <y>`: one step of a touch.
fn touch<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Command> {
    Some(Command::Touch {
        phase: match words.next()? {
            "down" => Touch::Down,
            "move" => Touch::Move,
            "up" => Touch::Up,
            _ => return None,
        },
        x: words.next()?.parse().ok()?,
        y: words.next()?.parse().ok()?,
    })
}

/// `drag <x> <y> <x> <y> <steps> <ms>`: a touch drawn from one point to
/// another.
fn drag<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Command> {
    let mut number = || words.next()?.parse::<u16>().ok();
    let (from, to) = ((number()?, number()?), (number()?, number()?));
    let (steps, ms) = (number()?, number()?);
    if steps == 0 {
        return None;
    }
    Some(Command::Drag {
        from,
        to,
        steps,
        ms,
    })
}

/// `clicks <button> <count> <every> <held> [jitter]`: presses one after
/// the other.
fn clicks<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Command> {
    let name = words.next()?;
    let button = *crate::buttons::ALL.iter().find(|b| **b == name)?;
    let mut number = || words.next()?.parse::<u16>().ok();
    let (count, every, held) = (number()?, number()?, number()?);
    let jitter = match words.next() {
        Some(word) => word.parse::<u16>().ok()?,
        None => 0,
    };
    if held == 0 || u32::from(held) + u32::from(jitter) >= u32::from(every) {
        return None;
    }
    Some(Command::Clicks {
        button,
        count,
        every,
        held,
        jitter,
    })
}

/// `true` for `on`, `false` for `off`, nothing for any other word.
fn on_off(word: &str, on: &str, off: &str) -> Option<bool> {
    match word {
        w if w == on => Some(true),
        w if w == off => Some(false),
        _ => None,
    }
}

/// The X4's button of that name.
fn button(name: &str) -> Option<&'static str> {
    buttons::ALL.iter().find(|b| **b == name).copied()
}

/// Bytes written as hexadecimal, two digits each.
pub fn hex_bytes(hex: &[u8]) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    hex.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |c: u8| char::from(c).to_digit(16);
            Some((digit(pair[0])? << 4 | digit(pair[1])?) as u8)
        })
        .collect()
}

/// The rest of a line's `key=hex value` pairs, if all are.
fn pairs<'a>(words: &mut impl Iterator<Item = &'a str>) -> Option<Vec<(String, String)>> {
    words
        .map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((String::from(key), unhex(value)?))
        })
        .collect()
}

/// Text sent as hexadecimal (`-` for none), so that it may hold spaces.
pub fn unhex(word: &str) -> Option<String> {
    if word == "-" {
        return Some(String::new());
    }
    String::from_utf8(hex_bytes(word.as_bytes())?).ok()
}

/// `bytes` as runs of one byte, written in hexadecimal: a count from 1 to
/// 255, then the byte.
pub fn runs(bytes: &[u8], out: &mut String) {
    let mut rest = bytes;
    while let Some(&byte) = rest.first() {
        let count = rest.iter().take(255).take_while(|b| **b == byte).count();
        let _ = write!(out, "{count:02x}{byte:02x}");
        rest = &rest[count..];
    }
}

/// The state of a part `describe` sends: `focused`, `on` or `off` and
/// `disabled` joined by `+`, or `-` for none.
pub fn part_state(focused: bool, on: Option<bool>, disabled: bool) -> String {
    let words = [
        (focused, "focused"),
        (on == Some(true), "on"),
        (on == Some(false), "off"),
        (disabled, "disabled"),
    ];
    let mut state = String::new();
    for (_, word) in words.iter().filter(|(on, _)| *on) {
        if !state.is_empty() {
            state.push('+');
        }
        state.push_str(word);
    }
    if state.is_empty() {
        state.push('-');
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The X4's log of the first Right on Home after a start (#335): the
    /// press came at 19,295 ms and showed a frame that waited (the covers),
    /// done at 19,877; a long press is 500 ms. The click is let go before
    /// the kernel's tick that follows the frame, a hold stays down.
    #[test]
    fn a_press_is_let_go_by_its_own_time_whatever_its_frame_took() {
        let (pressed, frame_done, long_press) = (19_295, 19_877, 500);
        let mut hold = Hold::default();
        assert!(!hold.is_down() && !hold.due(frame_done));
        hold.press("right", pressed, PRESS_MS);
        assert!(hold.is_down());
        assert!(!hold.due(pressed + PRESS_MS - 1));
        assert!(hold.due(pressed + PRESS_MS));
        assert!(hold.due(frame_done) && frame_done >= pressed + long_press);
        assert_eq!(hold.take(), Some("right"));
        assert_eq!(hold.take(), None);
        // Held on purpose: down until its time, a long press meanwhile.
        hold.press("right", pressed, 1500);
        assert!(!hold.due(frame_done));
        assert!(hold.due(pressed + 1500));
    }

    #[test]
    fn presses_and_holds() {
        assert_eq!(
            parse("press confirm"),
            Some(Command::Press {
                button: "confirm",
                ms: PRESS_MS
            })
        );
        assert_eq!(
            parse(" hold power 1500 "),
            Some(Command::Press {
                button: "power",
                ms: 1500
            })
        );
        assert_eq!(parse("press nothing"), None);
        assert_eq!(parse("press confirm now"), None);
    }

    #[test]
    fn paths_keep_their_spaces() {
        assert_eq!(
            parse("get /books/a book.epub"),
            Some(Command::Get(String::from("/books/a book.epub")))
        );
        assert_eq!(
            parse("put 12 /books/a book.epub"),
            Some(Command::Put {
                size: 12,
                path: String::from("/books/a book.epub")
            })
        );
        assert_eq!(parse("get"), None);
        assert_eq!(parse("put 12"), None);
        assert_eq!(
            parse("open 2f612062"),
            Some(Command::Open(String::from("/a b")))
        );
        assert_eq!(
            parse("set core.font-size 3132"),
            Some(Command::Set {
                id: String::from("core.font-size"),
                value: String::from("12")
            })
        );
        assert_eq!(
            parse("set core.time-zone-rule"),
            Some(Command::Set {
                id: String::from("core.time-zone-rule"),
                value: String::new()
            })
        );
        assert_eq!(
            parse("setting core.night-mode"),
            Some(Command::Setting(String::from("core.night-mode")))
        );
    }

    #[test]
    fn networks_in_hexadecimal() {
        assert_eq!(
            parse("wifi connect 4e6574 -"),
            Some(Command::Network(Ask::Connect {
                ssid: String::from("Net"),
                password: String::new()
            }))
        );
        assert_eq!(parse("wifi connect 4e6 -"), None);
        assert_eq!(parse("wifi connect zz -"), None);
        assert_eq!(
            parse("http get http://example.org/ /a"),
            Some(Command::Network(Ask::Get {
                url: String::from("http://example.org/"),
                save_to: Some(String::from("/a"))
            }))
        );
    }

    #[test]
    fn peeks_only_into_the_rom() {
        assert_eq!(
            parse("peek 40000000 16"),
            Some(Command::Peek {
                at: 0x4000_0000,
                words: 16
            })
        );
        assert_eq!(parse("peek 40000002 1"), None);
        assert_eq!(parse("peek 3ffffffc 1"), None);
        assert_eq!(parse("peek 4005fffc 2"), None);
        assert_eq!(parse("peek 40000000 0"), None);
        assert_eq!(parse("peek ffffffff 4096"), None);
    }

    #[test]
    fn switches_and_unknown_lines() {
        assert_eq!(parse("refresh full"), Some(Command::Refresh { full: true }));
        assert_eq!(parse("trace stop"), Some(Command::Trace(false)));
        assert_eq!(parse("idle sleep 30"), Some(Command::IdleSleep(30)));
        assert_eq!(parse("refresh half"), None);
        assert_eq!(parse("dance"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn the_storage() {
        assert_eq!(parse("ls"), Some(Command::List(String::new())));
        assert_eq!(parse("ls /"), Some(Command::List(String::new())));
        assert_eq!(
            parse("stat /books/a.epub"),
            Some(Command::Stat(String::from("books/a.epub")))
        );
        assert_eq!(
            parse("ls books/my books"),
            Some(Command::List(String::from("books/my books")))
        );
        assert_eq!(
            parse("rm updates/a b.bin"),
            Some(Command::Remove(String::from("updates/a b.bin")))
        );
        assert_eq!(parse("rm"), None);
        assert_eq!(parse("rm  "), None);
        assert_eq!(
            parse("mkdir a/b"),
            Some(Command::MakeDir(String::from("a/b")))
        );
        assert_eq!(parse("md5 a"), Some(Command::Md5(String::from("a"))));
        assert_eq!(parse("stat a"), Some(Command::Stat(String::from("a"))));
        assert_eq!(
            parse("mv 61 2f62"),
            Some(Command::Move {
                from: String::from("a"),
                to: String::from("/b")
            })
        );
        assert_eq!(parse("mv 61"), None);
    }

    #[test]
    fn what_runs_and_changes() {
        assert_eq!(parse("actions"), Some(Command::Actions(String::new())));
        assert_eq!(
            parse("settings reader."),
            Some(Command::Settings(String::from("reader.")))
        );
        assert_eq!(parse("time"), Some(Command::Time));
        assert_eq!(
            parse("time set 1760000000000"),
            Some(Command::SetTime(1_760_000_000_000))
        );
        assert_eq!(parse("time set soon"), None);
        assert_eq!(parse("log debug"), Some(Command::Log("debug")));
        assert_eq!(parse("log loud"), None);
        assert_eq!(
            parse("touch down 10 20"),
            Some(Command::Touch {
                phase: Touch::Down,
                x: 10,
                y: 20
            })
        );
        assert_eq!(parse("touch tap 1 2"), None);
        assert_eq!(
            parse("drag 400 600 300 200 40 10"),
            Some(Command::Drag {
                from: (400, 600),
                to: (300, 200),
                steps: 40,
                ms: 10,
            })
        );
        assert_eq!(parse("drag 1 2 3 4 0 10"), None);
        assert_eq!(parse("drag 1 2 3"), None);
        assert_eq!(
            parse("emit core.book-opened path=2f61"),
            Some(Command::Emit {
                event: String::from("core.book-opened"),
                params: alloc::vec![(String::from("path"), String::from("/a"))]
            })
        );
        assert_eq!(parse("emit e path"), None);
        assert_eq!(
            parse("type 4772c3bc"),
            Some(Command::Type {
                text: String::from("Grü"),
                done: false
            })
        );
        assert_eq!(
            parse("type-enter -"),
            Some(Command::Type {
                text: String::new(),
                done: true
            })
        );
        assert_eq!(parse("type"), None);
        assert_eq!(parse("type 4g"), None);
        assert_eq!(
            parse("slot core.settings id=726561646572"),
            Some(Command::Slot {
                id: String::from("core.settings"),
                params: alloc::vec![(String::from("id"), String::from("reader"))]
            })
        );
        assert_eq!(
            parse("show calibre.screen"),
            Some(Command::Show {
                id: String::from("calibre.screen"),
                params: alloc::vec![]
            })
        );
        assert_eq!(parse("slot"), None);
        assert_eq!(parse("show s key=zz"), None);
        assert_eq!(
            parse("panic 6f68"),
            Some(Command::Panic(String::from("oh")))
        );
        assert_eq!(parse("restart now"), None);
    }

    #[test]
    fn clicks_and_their_gaps() {
        assert_eq!(
            parse("clicks right 30 800 100"),
            Some(Command::Clicks {
                button: crate::buttons::ALL[3],
                count: 30,
                every: 800,
                held: 100,
                jitter: 0,
            })
        );
        assert_eq!(
            parse("clicks right 20 800 100 500"),
            Some(Command::Clicks {
                button: crate::buttons::ALL[3],
                count: 20,
                every: 800,
                held: 100,
                jitter: 500,
            })
        );
        assert_eq!(parse("clicks right 20 800 100 700"), None);
        // The gaps of a seed, as tools reckon them too.
        let mut seed = 1;
        let gaps: Vec<u16> = (0..4).map(|_| click_gap(800, 500, &mut seed)).collect();
        assert_eq!(gaps, [1122, 788, 403, 533]);
        assert_eq!(parse("clicks right 30 100 100"), None);
        assert_eq!(parse("clicks nose 30 800 100"), None);
    }

    #[test]
    fn rows_go_as_runs() {
        let mut out = String::new();
        runs(&[0xFF, 0xFF, 0xFF, 0x00], &mut out);
        assert_eq!(out, "03ff0100");
        out.clear();
        runs(&[7; 300], &mut out);
        assert_eq!(out, "ff072d07");
    }

    #[test]
    fn data_lines() {
        assert_eq!(hex_bytes(b"00ff10"), Some(alloc::vec![0, 0xFF, 0x10]));
        assert_eq!(hex_bytes(b"0g"), None);
        assert_eq!(hex_bytes(b"001"), None);
    }

    #[test]
    fn states_of_parts() {
        assert_eq!(part_state(false, None, false), "-");
        assert_eq!(part_state(true, Some(true), false), "focused+on");
        assert_eq!(part_state(false, Some(false), false), "off");
        assert_eq!(part_state(true, None, true), "focused+disabled");
    }
}
