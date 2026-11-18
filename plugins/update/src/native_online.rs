//! The online flow without the host: what the update server is asked (the
//! firmware, the plugins built in and the code plugins installed, and what
//! counts devices without knowing them), how its answer is read, and the update
//! it names, kept in `offer.txt` (the interface-6 file, `docs/data-files.md`).
//! Asking, downloading and the page are the plugin's (`native_plugin/`).
//!
//! The question is a `POST` to `<server>/api/v1/updates`:
//! `{"project": …, "chip": 5, "version": "0.4.0", "plugins": ["paperpaw-plugin-stats
//! 0.4.0", …], "code": ["hello 0.1.0", …], "abi_major": 7, "abi_draft": 1,
//! "trigger": "manual", "first": ["day"], "age": "new"}`. The answer is read as
//! it streams in pieces; nothing of it is held whole.
use crate::native_update::{Periods, age, first, is_pre_release, is_version};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    firmware::compare_versions,
    install::{Digest, Package, PackageFile},
    json::stream::{Event as Json, Parser},
    native::{Error, net::Reason},
    time::{civil_from_days, days_from_civil},
};
use paperpaw_ui_kit::text::{copy, write_to};

/// The update server PaperPaw asks unless the setting says otherwise.
pub const DEFAULT_SERVER: &str = "https://paperpaw.org";
/// The update the server named last, kept until it is downloaded or no longer newer.
pub const OFFER: &str = "offer.txt";
/// The most the kept offer is read with: far more than its lines for 24 builds.
pub const MAX_OFFER: usize = 24 * 1024;
/// The question, written to a file as it is over what a request carries inline.
pub const QUESTION: &str = "question.json";
/// The date of the last check and of the first, as `YYYY-MM-DD` (UTC; interface 6's files).
pub const LAST: &str = "last-check.txt";
pub const FIRST: &str = "first-check.txt";
/// Lines of one list of the answer kept at most.
const MAX_LINES: usize = 128;
/// Builds of code plugins kept at most, and files of one build.
const MAX_BUILDS: usize = 24;
const MAX_FILES: usize = 8;

/// A code plugin's build for the update: its version and its files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Build {
    pub package: Package,
    pub version: String,
}

/// An update the server named.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    /// A full address, or a path on the server.
    pub url: String,
    /// Its size in bytes, 0 where the server does not say.
    pub size: u64,
    /// The crates it has built in, a line each: `crate version`.
    pub plugins: String,
    /// Why crates it lacks are missing: `crate word` a line (`unknown`, `not-ready`).
    pub missing: String,
    /// The code plugins of the question with no build for it yet: `id version` a line.
    pub waiting: String,
    /// The builds for it of the code plugins this device has.
    pub builds: Vec<Build>,
}

impl Offer {
    /// The text of [`OFFER`]: the version, the address and the size a line
    /// each, then `plugin`, `missing` and `waiting` lines, then each build as
    /// `build <id> <version> <author or -> <name>` and a `file <size> <digest>
    /// <path> <url>` line for each of its files.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn to_text(&self) -> Result<String, Error> {
        let mut text = String::new();
        write_to(
            &mut text,
            format_args!("{}\n{}\n{}\n", self.version, self.url, self.size),
        )?;
        for (tag, lines) in [
            ("plugin", &self.plugins),
            ("missing", &self.missing),
            ("waiting", &self.waiting),
        ] {
            for line in lines.lines() {
                write_to(&mut text, format_args!("{tag} {line}\n"))?;
            }
        }
        for build in &self.builds {
            write_to(
                &mut text,
                format_args!(
                    "build {} {} - {}\n",
                    build.package.plugin,
                    build.version,
                    build.package.name.replace(['\n', '\r'], " ")
                ),
            )?;
            for file in &build.package.files {
                write_to(
                    &mut text,
                    format_args!(
                        "file {} {} {} {}\n",
                        file.size,
                        file.digest.text(),
                        file.path,
                        file.url
                    ),
                )?;
            }
        }
        Ok(text)
    }

    /// What [`Offer::to_text`] wrote; lines it does not know are passed over.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; a text that is no offer is `Ok(None)`.
    pub fn from_text(text: &str) -> Result<Option<Self>, Error> {
        let mut lines = text.lines();
        let (Some(version), Some(url), Some(size)) = (lines.next(), lines.next(), lines.next())
        else {
            return Ok(None);
        };
        let Ok(size) = size.trim().parse() else {
            return Ok(None);
        };
        if !is_version(version.trim()) {
            return Ok(None);
        }
        let mut offer = Self {
            version: copy(version.trim())?,
            url: copy(url.trim())?,
            size,
            ..Self::default()
        };
        for line in lines {
            if let Some(rest) = line.strip_prefix("plugin ") {
                add_line(&mut offer.plugins, rest)?;
            } else if let Some(rest) = line.strip_prefix("missing ") {
                add_line(&mut offer.missing, rest)?;
            } else if let Some(rest) = line.strip_prefix("waiting ") {
                add_line(&mut offer.waiting, rest)?;
            } else if let Some(rest) = line.strip_prefix("build ") {
                let mut parts = rest.splitn(4, ' ');
                let (Some(id), Some(version), Some(_author)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    continue;
                };
                let name = parts.next().unwrap_or(id).trim();
                let package = Package::new(copy(id)?, copy(name)?);
                push_build(
                    &mut offer.builds,
                    Build {
                        package,
                        version: copy(version)?,
                    },
                )?;
            } else if let Some(rest) = line.strip_prefix("file ")
                && let Some(build) = offer.builds.last_mut()
                && let Some(file) = read_file(rest)?
                && build.package.files.len() < MAX_FILES
            {
                paperpaw_ui_kit::mem::push(&mut build.package.files, file)?;
            }
        }
        offer.builds.retain(|build| build.package.is_valid());
        Ok(Some(offer))
    }
}

/// `<size> <algorithm>:<hex> <path> <url>`.
fn read_file(rest: &str) -> Result<Option<PackageFile>, Error> {
    let mut parts = rest.splitn(4, ' ');
    let (Some(size), Some(digest), Some(path), Some(url)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Ok(None);
    };
    let digest = match digest.split_once(':') {
        Some(("sha256", hex)) => Digest::Sha256(copy(hex)?),
        Some((_, hex)) => Digest::Sha1(copy(hex)?),
        None => return Ok(None),
    };
    Ok(Some(PackageFile::new(
        copy(path)?,
        copy(url.trim())?,
        size.parse().unwrap_or(0),
        digest,
    )))
}

fn push_build(builds: &mut Vec<Build>, build: Build) -> Result<(), Error> {
    if builds.len() < MAX_BUILDS {
        paperpaw_ui_kit::mem::push(builds, build)?;
    }
    Ok(())
}

/// `line` and a line end added to `list`, fallibly, up to [`MAX_LINES`] lines.
fn add_line(list: &mut String, line: &str) -> Result<(), Error> {
    if list.lines().count() >= MAX_LINES {
        return Ok(());
    }
    list.try_reserve(line.len().saturating_add(1))
        .map_err(|_| Error::NoMemory)?;
    list.push_str(line.trim());
    list.push('\n');
    Ok(())
}

/// Reads the answer as it streams: the update it names and the builds.
#[derive(Debug, Default)]
pub struct Reader {
    parser: Parser,
    offer: Offer,
    /// A piece of the answer could not be kept for want of memory.
    full: bool,
}

impl Reader {
    /// The next piece of the answer's body.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn feed(&mut self, piece: &[u8]) -> Result<(), Error> {
        let Self {
            parser,
            offer,
            full,
        } = self;
        parser
            .try_feed(piece, &mut |event| *full |= note(offer, event).is_err())
            .map_err(|_| Error::NoMemory)?;
        if self.full {
            return Err(Error::NoMemory);
        }
        Ok(())
    }

    /// The update the answer names, if it names one newer than `running`, and
    /// a pre-release only where the reader asked for `betas` or the device runs
    /// one, whatever the server sends.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn finish(mut self, running: &str, betas: bool) -> Result<Option<Offer>, Error> {
        let Self {
            parser,
            offer,
            full,
        } = &mut self;
        parser
            .try_finish(&mut |event| *full |= note(offer, event).is_err())
            .map_err(|_| Error::NoMemory)?;
        if self.full {
            return Err(Error::NoMemory);
        }
        let mut offer = self.offer;
        offer.version = copy(offer.version.trim())?;
        offer.url = copy(offer.url.trim())?;
        let named = is_version(&offer.version)
            && !offer.url.is_empty()
            && compare_versions(&offer.version, running).is_gt()
            && (!is_pre_release(&offer.version) || betas || is_pre_release(running));
        offer.builds.retain(|build| {
            build.package.is_valid() && !build.version.contains(char::is_whitespace)
        });
        Ok(named.then_some(offer))
    }
}

/// Takes what an event of the answer says into `offer`.
fn note(offer: &mut Offer, event: Json<'_>) -> Result<(), Error> {
    match event {
        Json::Open(path) if path == ["code", "[]"] => {
            push_build(&mut offer.builds, Build::default())
        }
        Json::Open(path) if path == ["code", "[]", "files", "[]"] => {
            if let Some(build) = offer.builds.last_mut()
                && build.package.files.len() < MAX_FILES
            {
                paperpaw_ui_kit::mem::push(&mut build.package.files, PackageFile::default())?;
            }
            Ok(())
        }
        Json::Value(path, text) => match path {
            [key] => top(offer, key, text),
            [list, item] if item == "[]" => match list.as_str() {
                "plugins" => add_line(&mut offer.plugins, text),
                "missing" => add_line(&mut offer.missing, text),
                "waiting" => add_line(&mut offer.waiting, text),
                _ => Ok(()),
            },
            [code, item, key] if code == "code" && item == "[]" => build_member(offer, key, text),
            [code, item, files, entry, key]
                if code == "code" && item == "[]" && files == "files" && entry == "[]" =>
            {
                file_member(offer, key, text)
            }
            _ => Ok(()),
        },
        _ => Ok(()),
    }
}

fn top(offer: &mut Offer, key: &str, text: &str) -> Result<(), Error> {
    match key {
        "version" => offer.version = copy(text)?,
        "url" => offer.url = copy(text)?,
        "size" => offer.size = text.trim().parse().unwrap_or(0),
        _ => {}
    }
    Ok(())
}

fn build_member(offer: &mut Offer, key: &str, text: &str) -> Result<(), Error> {
    let Some(build) = offer.builds.last_mut() else {
        return Ok(());
    };
    match key {
        "id" => build.package.plugin = copy(text)?,
        "name" => build.package.name = copy(text)?,
        "version" => build.version = copy(text)?,
        _ => {}
    }
    Ok(())
}

fn file_member(offer: &mut Offer, key: &str, text: &str) -> Result<(), Error> {
    let Some(file) = offer
        .builds
        .last_mut()
        .and_then(|build| build.package.files.last_mut())
    else {
        return Ok(());
    };
    match key {
        "path" => file.path = copy(text)?,
        "url" => file.url = copy(text)?,
        "size" => file.size = text.trim().parse().unwrap_or(0),
        key => {
            file.digest.take(key, text);
        }
    }
    Ok(())
}

/// What the question says of the firmware, in the order it is written.
#[derive(Debug, Clone, Copy)]
pub struct Firmware<'a> {
    pub project: &'a str,
    pub chip: u32,
    pub version: &'a str,
}

/// Appends `text` as a JSON string.
fn quote(out: &mut String, text: &str) -> Result<(), Error> {
    out.try_reserve(text.len().saturating_add(2))
        .map_err(|_| Error::NoMemory)?;
    out.push('"');
    for c in text.chars().filter(|c| !c.is_control()) {
        if c == '"' || c == '\\' {
            out.try_reserve(1).map_err(|_| Error::NoMemory)?;
            out.push('\\');
        }
        out.try_reserve(c.len_utf8()).map_err(|_| Error::NoMemory)?;
        out.push(c);
    }
    out.try_reserve(1).map_err(|_| Error::NoMemory)?;
    out.push('"');
    Ok(())
}

/// The question as it is built: the device, then the lists, then the counting.
#[derive(Debug, Default)]
pub struct Question {
    body: String,
    plugins: usize,
    code: usize,
}

impl Question {
    /// Starts it with what runs; `plugins` follows.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(device: &Firmware<'_>) -> Result<Self, Error> {
        let mut body = String::new();
        body.try_reserve(2048).map_err(|_| Error::NoMemory)?;
        write_to(&mut body, format_args!("{{\"project\":"))?;
        quote(&mut body, device.project)?;
        write_to(
            &mut body,
            format_args!(",\"chip\":{},\"version\":", device.chip),
        )?;
        quote(&mut body, device.version)?;
        write_to(&mut body, format_args!(",\"plugins\":["))?;
        Ok(Self {
            body,
            ..Self::default()
        })
    }

    /// A plugin built in now, named as its crate is (`paperpaw-plugin-<id>`).
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn built_in(&mut self, id: &str, version: &str) -> Result<(), Error> {
        let comma = if self.plugins == 0 { "" } else { "," };
        write_to(&mut self.body, format_args!("{comma}"))?;
        let mut crate_line = String::new();
        write_to(
            &mut crate_line,
            format_args!("paperpaw-plugin-{id} {version}"),
        )?;
        quote(&mut self.body, &crate_line)?;
        self.plugins += 1;
        Ok(())
    }

    /// Closes the plugins; the code plugins installed come next.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn code_begin(&mut self) -> Result<(), Error> {
        write_to(&mut self.body, format_args!("],\"code\":["))
    }

    /// A code plugin installed, with its version.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn code(&mut self, id: &str, version: &str) -> Result<(), Error> {
        let comma = if self.code == 0 { "" } else { "," };
        write_to(&mut self.body, format_args!("{comma}"))?;
        let mut line = String::new();
        write_to(&mut line, format_args!("{id} {version}"))?;
        quote(&mut self.body, &line)?;
        self.code += 1;
        Ok(())
    }

    /// Ends it: the interface the code plugins must be built for, the betas the
    /// reader asked for, and the counting. The body.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn finish(
        mut self,
        interface: Option<(u32, u32)>,
        betas: bool,
        counting: &Counting,
    ) -> Result<String, Error> {
        write_to(&mut self.body, format_args!("]"))?;
        if let Some((major, draft)) = interface {
            write_to(
                &mut self.body,
                format_args!(",\"abi_major\":{major},\"abi_draft\":{draft}"),
            )?;
        }
        if betas {
            write_to(&mut self.body, format_args!(",\"betas\":true"))?;
        }
        write_to(
            &mut self.body,
            format_args!(",\"trigger\":\"{}\"", counting.trigger),
        )?;
        if let Some(periods) = counting.periods {
            write_to(&mut self.body, format_args!(",\"first\":["))?;
            for (index, name) in periods.names().enumerate() {
                let comma = if index == 0 { "" } else { "," };
                write_to(&mut self.body, format_args!("{comma}\"{name}\""))?;
            }
            write_to(&mut self.body, format_args!("]"))?;
        }
        if let Some(age) = counting.age {
            write_to(&mut self.body, format_args!(",\"age\":\"{age}\""))?;
        }
        write_to(&mut self.body, format_args!("}}"))?;
        Ok(self.body)
    }
}

/// How a check counts the device without knowing it: what started it, the
/// periods it is the first check of, and how long the device has been in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Counting {
    pub trigger: &'static str,
    pub periods: Option<Periods>,
    pub age: Option<&'static str>,
}

/// The dates to keep after a check: the last (always when later) and the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Keep {
    pub last: Option<i64>,
    pub first: Option<i64>,
}

/// The counting of a check on `today` (days since 1970, where the time is
/// known), from the dates kept: `last` (the last check) and `kept` (the first).
/// The first check of a device is the day of its last, else today; a first
/// after today (the clock went back) is taken as today.
#[must_use]
pub fn counting(
    auto: bool,
    today: Option<i64>,
    last: Option<i64>,
    kept: Option<i64>,
) -> (Counting, Keep) {
    let trigger = if auto { "auto" } else { "manual" };
    let Some(today) = today else {
        let counting = Counting {
            trigger,
            periods: None,
            age: None,
        };
        return (
            counting,
            Keep {
                last: None,
                first: None,
            },
        );
    };
    let kept = kept.filter(|&kept| kept <= today);
    let first_day = kept.unwrap_or_else(|| last.filter(|&last| last <= today).unwrap_or(today));
    let counting = Counting {
        trigger,
        periods: Some(first(last, today)),
        age: Some(age(first_day, today)),
    };
    let keep = Keep {
        last: last.is_none_or(|last| today > last).then_some(today),
        first: (kept != Some(first_day)).then_some(first_day),
    };
    (counting, keep)
}

/// A date kept as `YYYY-MM-DD`, in days since 1970.
#[must_use]
pub fn parse_date(bytes: &[u8]) -> Option<i64> {
    let text = core::str::from_utf8(bytes).ok()?.trim();
    let mut parts = text.splitn(3, '-').map(str::parse::<u32>);
    let (Some(Ok(year)), Some(Ok(month)), Some(Ok(day))) =
        (parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    let (month, day) = (u8::try_from(month).ok()?, u8::try_from(day).ok()?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(i32::try_from(year).ok()?, month, day))
}

/// `days` since 1970 as `YYYY-MM-DD`.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn date_text(days: i64) -> Result<String, Error> {
    let (year, month, day) = civil_from_days(days);
    let mut text = String::new();
    write_to(&mut text, format_args!("{year:04}-{month:02}-{day:02}\n"))?;
    Ok(text)
}

/// Why the update server could not be asked, or what it answered instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trouble {
    /// It answered with this error status.
    Status(u32),
    /// The request failed: the text that says why.
    Reason(&'static str),
}

impl Trouble {
    /// What a request that did not bring a 2xx answer says: its status where
    /// one came, else the reason it ended with.
    #[must_use]
    pub const fn of(reason: Reason, status: u32) -> Self {
        Self::Reason(match reason {
            Reason::Ok => return Self::Status(status),
            Reason::NoNetwork => "update.net.no-network",
            Reason::Dns | Reason::Refused => "update.net.refused",
            Reason::Tls => "update.net.tls",
            Reason::Timeout => "update.net.timeout",
            Reason::Offline | Reason::Saving | Reason::NetworkOff => "update.net.held",
            _ => "update.net.other",
        })
    }
}

/// The server's address: the setting, without a slash at the end, or the default.
#[must_use]
pub fn server(setting: &str) -> &str {
    let trimmed = setting.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_SERVER
    } else {
        trimmed
    }
}

/// The server's name as the reader knows it, e.g. `paperpaw.org`.
#[must_use]
pub fn server_name(server: &str) -> &str {
    let name = server.split_once("://").map_or(server, |(_, rest)| rest);
    name.split('/').next().unwrap_or(name)
}

/// Whether an update file of `size` bytes (its image and the signature
/// trailer after it) can go into an app slot of `room` bytes. A size the
/// server did not say (0) and a room the device did not say (0: no slot of its
/// own, as beside another firmware) are not a reason to refuse here: the device
/// checks again before it writes (`Update::fits`).
#[must_use]
pub fn fits_slot(size: u64, room: u64) -> bool {
    size == 0
        || room == 0
        || size.saturating_sub(paperpaw_sdk::firmware::TRAILER_LEN as u64) <= room
}

/// The address of a file the server names: a path on the server is joined.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn address(server: &str, url: &str) -> Result<String, Error> {
    if url.starts_with('/') {
        let mut full = String::new();
        write_to(&mut full, format_args!("{server}{url}"))?;
        Ok(full)
    } else {
        copy(url)
    }
}

#[cfg(test)]
#[path = "native_online_tests.rs"]
mod tests;
