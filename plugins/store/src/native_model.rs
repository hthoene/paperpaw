//! The plugin store's rules for the native plugin, without host calls: the
//! library's groups and limits, the line an index keeps of each plugin, what a
//! build needs of the firmware, the addresses asked for, the revocations a
//! library names. They are the interface-6 code made fallible (a refused
//! allocation is `NoMemory`, never an abort) and free of host state, tested
//! to give the same answers as the old code. Everything here is read from a
//! library nobody reviewed (community plugins too): every text is cut or
//! refused at a limit named in this file, and nothing indexes text or bytes
//! in a way that can panic.
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_sdk::digest::hex;
use paperpaw_sdk::native::{Error, Fingerprint, INTERFACE, keys::MAX_REVOKED};
use paperpaw_ui_kit::{
    mem::push,
    text::{concat, copy, decimal_u64},
};

/// The format of the list and of a plugin's page this plugin reads.
pub const FORMAT: u32 = 2;
/// The longest summary an index line may hold, in bytes, and the longest name
/// (and id, author, licence, version, SDK): the store writes lines within them,
/// so a longer one is not the store's and is not read.
pub const MAX_SUMMARY: usize = 300;
pub const MAX_NAME: usize = 80;
/// The longest line of the index read; a longer one is not a line.
pub const MAX_LINE: usize = 1024;
/// Runs of plugins of one group at most.
pub const MAX_RUNS: usize = 32;
/// The plugins of a section kept at most.
pub const MAX_SECTION: usize = 20;

/// PaperPaw's own plugin library: the only list whose word that a plugin is
/// PaperPaw's is taken.
pub const OFFICIAL_LIST: &str = "https://paperpaw.org/api/v1/plugins.json";

/// The plugins a library picks for a start (its `sections`), in the order the
/// start shows them.
pub const SECTIONS: [&str; 5] = ["recommended", "new", "trending", "updated", "discover"];

/// The groups, in the order the website shows them. `system` stays last: it
/// takes a plugin of a group this plugin does not know.
pub const GROUPS: [&str; 13] = [
    "reading",
    "formats",
    "library",
    "sync",
    "home",
    "look",
    "notes",
    "learning",
    "news",
    "tools",
    "games",
    "languages",
    "system",
];

/// The group of `name`, the last (`system`) for one this plugin does not know.
#[must_use]
pub fn group(name: &str) -> usize {
    GROUPS
        .iter()
        .position(|g| *g == name)
        .unwrap_or(GROUPS.len() - 1)
}

/// Text under construction that never grows without asking.
struct Out(String);

impl Out {
    fn text(&mut self, text: &str) -> Result<(), Error> {
        self.0
            .try_reserve(text.len())
            .map_err(|_| Error::NoMemory)?;
        self.0.push_str(text);
        Ok(())
    }

    fn ch(&mut self, c: char) -> Result<(), Error> {
        self.0
            .try_reserve(c.len_utf8())
            .map_err(|_| Error::NoMemory)?;
        self.0.push(c);
        Ok(())
    }

    fn number(&mut self, n: u64) -> Result<(), Error> {
        let mut digits = [0; 20];
        self.text(decimal_u64(n, &mut digits))
    }

    /// `text` with tabs and line ends made spaces.
    fn clean(&mut self, text: &str) -> Result<(), Error> {
        self.0
            .try_reserve(text.len())
            .map_err(|_| Error::NoMemory)?;
        for c in text.chars() {
            self.0.push(if matches!(c, '\t' | '\n' | '\r') {
                ' '
            } else {
                c
            });
        }
        Ok(())
    }
}

/// The author's key an id names, where the library knows one.
pub type AuthorFn<'a> = dyn Fn(&str) -> Option<[u8; 32]> + 'a;
/// The plugins already stopped.
pub type StoppedFn<'a> = dyn Fn() -> Vec<String> + 'a;

/// A plugin the library offers for this device: a data plugin, or a code
/// plugin with a build for this PaperPaw.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entry {
    pub id: String,
    /// In the UI's language where the library has it.
    pub name: String,
    pub summary: String,
    /// Index into [`GROUPS`].
    pub group: usize,
    pub author: String,
    pub licence: String,
    pub version: String,
    /// The PaperPaw (its SDK) it is made for, e.g. `0.4`.
    pub sdk: String,
    /// It has code, which runs with full access to the device.
    pub code: bool,
    /// One of PaperPaw's own.
    pub official: bool,
    /// A community plugin, which nobody reviewed.
    pub unreviewed: bool,
    /// Its newest build is made for a newer PaperPaw than this device runs.
    pub needs_newer: bool,
    /// The library has builds of it for this device, but none for the PaperPaw
    /// that runs.
    pub waits_author: bool,
    /// Bytes to download, its code with them.
    pub size: u64,
    /// When it was first published and when its newest version, as `yyyymmdd`
    /// (0 where the library does not say).
    pub added: u32,
    pub updated: u32,
}

impl Entry {
    /// Its line in the index, tabs and line ends of the list left out.
    ///
    /// # Errors
    /// `NoMemory` when the line cannot be kept.
    pub fn line(&self) -> Result<String, Error> {
        let mut out = Out(String::new());
        let flags = u8::from(self.code)
            | (u8::from(self.official) << 1)
            | (u8::from(self.unreviewed) << 2)
            | (u8::from(self.needs_newer) << 3)
            | (u8::from(self.waits_author) << 4);
        for number in [
            self.group as u64,
            self.size,
            u64::from(flags),
            u64::from(self.added),
            u64::from(self.updated),
        ] {
            out.number(number)?;
            out.ch('\t')?;
        }
        for (at, text) in [
            &self.id,
            &self.version,
            &self.sdk,
            &self.author,
            &self.licence,
            &self.name,
            &self.summary,
        ]
        .into_iter()
        .enumerate()
        {
            if at > 0 {
                out.ch('\t')?;
            }
            out.clean(text)?;
        }
        out.ch('\n')?;
        Ok(out.0)
    }

    /// The entry a line of the index holds; `None` for a line that is none.
    ///
    /// # Errors
    /// `NoMemory` when the entry cannot be kept.
    pub fn parse(line: &str) -> Result<Option<Self>, Error> {
        if line.len() > MAX_LINE {
            return Ok(None);
        }
        let mut f = line.splitn(12, '\t');
        let (Some(group), Some(size), Some(flags)) = (f.next(), f.next(), f.next()) else {
            return Ok(None);
        };
        let (Ok(group), Ok(size), Ok(flags)) = (
            group.parse::<usize>(),
            size.parse::<u64>(),
            flags.parse::<u8>(),
        ) else {
            return Ok(None);
        };
        let (Some(added), Some(updated)) = (f.next(), f.next()) else {
            return Ok(None);
        };
        let (Ok(added), Ok(updated)) = (added.parse::<u32>(), updated.parse::<u32>()) else {
            return Ok(None);
        };
        let (
            Some(id),
            Some(version),
            Some(sdk),
            Some(author),
            Some(licence),
            Some(name),
            Some(summary),
        ) = (
            f.next(),
            f.next(),
            f.next(),
            f.next(),
            f.next(),
            f.next(),
            f.next(),
        )
        else {
            return Ok(None);
        };
        if [id, version, sdk, author, licence, name]
            .iter()
            .any(|t| t.len() > MAX_NAME)
            || summary.len() > MAX_SUMMARY
        {
            return Ok(None);
        }
        Ok(Some(Self {
            group,
            size,
            code: flags & 1 != 0,
            official: flags & 2 != 0,
            unreviewed: flags & 4 != 0,
            needs_newer: flags & 8 != 0,
            waits_author: flags & 16 != 0,
            added,
            updated,
            id: copy(id)?,
            version: copy(version)?,
            sdk: copy(sdk)?,
            author: copy(author)?,
            licence: copy(licence)?,
            name: copy(name)?,
            summary: copy(summary)?,
        }))
    }
}

/// Counts one more plugin of `group` in `runs` (a run is plugins of one group
/// in a row, with its count). Past [`MAX_RUNS`] runs the last one grows
/// instead, so the list is bounded however it is ordered.
///
/// # Errors
/// `NoMemory` when the run cannot be kept.
pub fn add_to_run(runs: &mut Vec<(usize, u32)>, group: usize) -> Result<(), Error> {
    let full = runs.len() >= MAX_RUNS;
    match runs.last_mut() {
        Some((last, count)) if *last == group || full => {
            *count = count.saturating_add(1);
            Ok(())
        }
        _ => push(runs, (group, 1)),
    }
}

/// Keeps `id` among the plugins of a section if it is a plugin's name and the
/// section holds fewer than [`MAX_SECTION`]; says whether it was kept.
///
/// # Errors
/// `NoMemory` when the id cannot be kept.
pub fn keep_in_section(ids: &mut Vec<String>, id: &str) -> Result<bool, Error> {
    if ids.len() >= MAX_SECTION || !paperpaw_sdk::install::is_plugin_name(id) {
        return Ok(false);
    }
    let id = copy(id)?;
    push(ids, id)?;
    Ok(true)
}

/// What a build's `abi` says it was made for: the fingerprint of format 4
/// (16 hexadecimal digits), or `<major>.<needs_minor>` of format 5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Abi {
    /// Format 4: the build's own, chip and draft included.
    Fingerprint(Fingerprint),
    /// Format 5: the interface major and the newest minor the build uses.
    Dotted {
        /// The interface major.
        major: u16,
        /// The newest minor of it the build needs.
        minor: u16,
    },
}

impl Abi {
    /// Either form; `None` for anything else (an empty part, a sign, a
    /// third number, leading zeros, a number past 16 bits).
    #[must_use]
    pub fn parse(text: &str) -> Option<Abi> {
        let text = text.trim();
        let Some((major, minor)) = text.split_once('.') else {
            return Fingerprint::parse(text).map(Abi::Fingerprint);
        };
        let number = |part: &str| {
            let plain = !part.is_empty()
                && part.len() <= 5
                && part.bytes().all(|b| b.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            plain.then(|| part.parse::<u16>().ok()).flatten()
        };
        Some(Abi::Dotted {
            major: number(major)?,
            minor: number(minor)?,
        })
    }

    /// The interface it names.
    #[must_use]
    pub const fn interface(self) -> u16 {
        match self {
            Abi::Fingerprint(f) => f.interface(),
            Abi::Dotted { major, .. } => major,
        }
    }

    /// The minor it needs, where it names one.
    #[must_use]
    pub const fn minor(self) -> Option<u16> {
        match self {
            Abi::Fingerprint(_) => None,
            Abi::Dotted { minor, .. } => Some(minor),
        }
    }

    /// Whether a build of this `abi` is made for the device: a fingerprint
    /// must be the device's own; a dotted one has no draft or chip of its own
    /// (the build's `device` is the target) and [`Provides::fit`] judges its
    /// major and minor.
    #[must_use]
    pub fn is_for(self, device: Option<Fingerprint>) -> bool {
        match self {
            Abi::Fingerprint(f) => device == Some(f),
            Abi::Dotted { .. } => true,
        }
    }
}

/// The families of shared functions a build or a firmware has.
pub const FAMILIES: usize = 8;

/// What a build says it needs, each only where the library names it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Needs {
    /// The plugin interface it was built for.
    pub interface: Option<u16>,
    /// The newest minor of that interface it uses (`needs_minor`), where the
    /// library names it as `7.<minor>`.
    pub minor: Option<u16>,
    /// The bytes of the firmware's table it uses.
    pub api: Option<u32>,
    /// The revision of each family it uses, 0 for the others; whole, or not
    /// taken at all (an odd list is not trusted for a part).
    pub families: Option<[u16; FAMILIES]>,
}

/// Whether a build runs on a firmware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// It does, or the library does not say it does not.
    Yes,
    /// It needs more of PaperPaw than this device has: a newer one.
    Newer,
    /// It was built for an older interface or before a change of a family it
    /// would see: it is for another PaperPaw, and not offered.
    Older,
}

/// What the firmware here provides.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provides {
    /// The newest minor of the interface it speaks (`Native::minor`, draft
    /// 29); `None` where it is not known yet: the minor is not checked.
    pub minor: Option<u16>,
    /// The bytes of its table.
    pub api: u32,
    /// The revision of each family it has (0: none).
    pub families: [u16; FAMILIES],
    /// The oldest revision of each family it still runs.
    pub oldest: [u16; FAMILIES],
    /// Whether the firmware checks families at all (the X4's chip does only).
    pub checks_families: bool,
}

impl Provides {
    /// How a build that `needs` this fits.
    #[must_use]
    pub fn fit(&self, needs: &Needs) -> Fit {
        let interface = needs.interface.unwrap_or(INTERFACE);
        if interface < INTERFACE {
            return Fit::Older;
        }
        let mut older = false;
        let mut newer = interface > INTERFACE
            || needs.api.is_some_and(|api| api > self.api)
            || needs
                .minor
                .zip(self.minor)
                .is_some_and(|(need, have)| need > have);
        let used = needs.families.filter(|_| self.checks_families);
        if let Some(used) = used {
            for (i, &rev) in used.iter().enumerate().filter(|(_, rev)| **rev != 0) {
                newer |= rev > self.families.get(i).copied().unwrap_or(0);
                older |= rev < self.oldest.get(i).copied().unwrap_or(0);
            }
        }
        // Newer wins where both hold, as the firmware says it.
        if newer {
            Fit::Newer
        } else if older {
            Fit::Older
        } else {
            Fit::Yes
        }
    }
}

/// `text` for a query: letters, digits and `-_.~` as they are, spaces as `+`,
/// everything else percent-encoded.
///
/// # Errors
/// `NoMemory` when the text cannot be kept.
pub fn encode(text: &str) -> Result<String, Error> {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = Out(String::new());
    out.0.try_reserve(text.len()).map_err(|_| Error::NoMemory)?;
    for byte in text.bytes() {
        match byte {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.ch(char::from(byte))?;
            }
            b' ' => out.ch('+')?,
            byte => {
                out.ch('%')?;
                out.ch(char::from(HEX[usize::from(byte >> 4)]))?;
                out.ch(char::from(HEX[usize::from(byte & 15)]))?;
            }
        }
    }
    Ok(out.0)
}

/// The address of the list at `list` for `device` in `language`, with the
/// community plugins where `community`, and the builds for the fingerprint
/// `abi` (as text) of the PaperPaw that runs.
///
/// # Errors
/// `NoMemory` when the address cannot be kept.
pub fn list_url(
    list: &str,
    device: &str,
    language: &str,
    community: bool,
    abi: Option<&str>,
) -> Result<String, Error> {
    let join = if list.contains('?') { "&" } else { "?" };
    let community = if community { "&community=1" } else { "" };
    let abi = match abi {
        Some(abi) => concat(&["&abi=", abi])?,
        None => String::new(),
    };
    concat(&[
        list,
        join,
        "device=",
        &encode(device)?,
        "&lang=",
        &encode(language)?,
        community,
        &abi,
    ])
}

/// The address of plugin `id`'s page, beside the list at `list` (the setting's,
/// without the device and the language) and with its query (a library may want
/// a key there), for `device` in `language`, a community plugin too where
/// `community`, and the builds for `abi` where it is named.
///
/// The page is `plugins/<id>.json` in the folder of the list's address, as
/// interface 6 joined it. **A difference on purpose:** an address whose path has
/// a `.` or `..` segment is refused (`Invalid`: the join here does not resolve
/// them, and no library has asked for it).
///
/// # Errors
/// `NoMemory` when the address cannot be kept; `Invalid` for an address that is
/// no `scheme://host/path` or whose path has dot segments.
pub fn plugin_url(
    list: &str,
    device: &str,
    language: &str,
    id: &str,
    community: bool,
    abi: Option<&str>,
) -> Result<String, Error> {
    let list = list.split('#').next().unwrap_or(list);
    let (address, query) = list.split_once('?').unwrap_or((list, ""));
    let (scheme, rest) = address.split_once("://").ok_or(Error::Invalid)?;
    let host_end = rest.find('/').unwrap_or(rest.len());
    let (host, path) = rest.split_at(host_end);
    if path.split('/').any(|segment| matches!(segment, "." | "..")) {
        return Err(Error::Invalid);
    }
    let dir = path
        .get(..path.rfind('/').map_or(0, |at| at + 1))
        .unwrap_or("");
    let dir = if dir.is_empty() { "/" } else { dir };
    let mut page = concat(&[scheme, "://", host, dir, "plugins/", &encode(id)?, ".json"])?;
    if !query.is_empty() {
        let mut joined = concat(&[&page, "?", query])?;
        core::mem::swap(&mut page, &mut joined);
    }
    list_url(&page, device, language, community, abi)
}

/// Entries of a list of revocations looked at, most: a bound on the reading,
/// apart from what is kept. Past it a list is not read further.
pub const MAX_READ: usize = 4096;

/// A revocation to hand over to the kernel, which checks the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revoked {
    pub plugin: String,
    pub key: [u8; 32],
    pub signature: [u8; 64],
}

/// The revocations of one list being read: the entries that could hold, of a
/// plugin on the device whose author's key they name and not stopped already.
/// At most [`MAX_REVOKED`], and only these count to that, so a list of entries
/// for plugins that are not here takes no place from those that are.
#[derive(Debug, Default)]
pub struct Revocations {
    plugin: String,
    key: String,
    signature: String,
    pending: Vec<Revoked>,
    /// The plugins already stopped, asked for when the first entry that could
    /// be new is met.
    kept: Option<Vec<String>>,
    /// Entries read, whatever they were.
    read: usize,
}

impl Revocations {
    /// Takes one field of the entry being read (`plugin`, `key`, `signature`);
    /// others are left. A longer value is cut one character past what a valid
    /// one holds, which no valid one can equal.
    ///
    /// # Errors
    /// `NoMemory` when the value cannot be kept.
    pub fn field(&mut self, field: &str, value: &str) -> Result<(), Error> {
        let (to, most) = match field {
            "plugin" => (&mut self.plugin, 64),
            "key" => (&mut self.key, 64),
            "signature" => (&mut self.signature, 128),
            _ => return Ok(()),
        };
        let cut = value
            .char_indices()
            .nth(most + 1)
            .map_or(value.len(), |(at, _)| at);
        *to = copy(value.get(..cut).unwrap_or(value))?;
        Ok(())
    }

    /// The entry is complete: kept if it is well formed, is of a plugin here
    /// (`here`) whose author's key it names (`author_of`; where `host_checks` the
    /// host keeps the pins and the entry goes to it whatever `author_of` says), and
    /// is new. `stopped`
    /// names the plugins already stopped, asked for only when an entry could
    /// be new.
    ///
    /// # Errors
    /// `NoMemory` when the entry cannot be kept.
    pub fn entry_done(
        &mut self,
        here: &dyn Fn(&str) -> bool,
        (author_of, stopped, host_checks): (&AuthorFn<'_>, &StoppedFn<'_>, bool),
    ) -> Result<(), Error> {
        let key = hex::decode_array::<32>(&self.key);
        let signature = hex::decode_array::<64>(&self.signature);
        let plugin = core::mem::take(&mut self.plugin);
        self.key.clear();
        self.signature.clear();
        self.read += 1;
        let (Some(key), Some(signature)) = (key, signature) else {
            return Ok(());
        };
        if self.read > MAX_READ
            || self.pending.len() >= MAX_REVOKED
            || plugin.is_empty()
            || !here(&plugin)
            || (!host_checks && author_of(&plugin) != Some(key))
            || self.pending.iter().any(|r| r.plugin == plugin)
        {
            return Ok(());
        }
        let kept = self.kept.get_or_insert_with(stopped);
        if !kept.contains(&plugin) {
            push(
                &mut self.pending,
                Revoked {
                    plugin,
                    key,
                    signature,
                },
            )?;
        }
        Ok(())
    }

    /// What is to be handed over to the kernel.
    #[must_use]
    pub fn pending(&self) -> &[Revoked] {
        &self.pending
    }
}

/// `href` made absolute with `base`, a page's address, as browsers resolve
/// links (`/a`, `../b`, `?q`, `//host/c`): the interface-6 `join_url` with a
/// refusal path (`NoMemory`), tested to give its answers.
///
/// # Errors
/// `NoMemory` when the address cannot be kept.
pub fn join_url(base: &str, href: &str) -> Result<String, Error> {
    if paperpaw_sdk::document::is_external(href) {
        return copy(href);
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return copy(href);
    };
    if let Some(net) = href.strip_prefix("//") {
        return concat(&[scheme, "://", net]);
    }
    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, after) = rest.split_at(host_end);
    let path = after.split(['?', '#']).next().unwrap_or("");
    let path = if path.is_empty() { "/" } else { path };
    let joined = if href.starts_with('/') {
        copy(href)?
    } else if href.starts_with('?') {
        concat(&[path, href])?
    } else if href.starts_with('#') {
        let without = after.split('#').next().unwrap_or("");
        concat(&[if without.is_empty() { "/" } else { without }, href])?
    } else {
        let dir = path
            .get(..path.rfind('/').map_or(0, |i| i + 1))
            .unwrap_or("");
        concat(&[dir, href])?
    };
    let path = normalize(&joined)?;
    concat(&[scheme, "://", host, &path])
}

/// Resolves `.` and `..` in the path of `target`, keeping its query and
/// fragment.
fn normalize(target: &str) -> Result<String, Error> {
    let split = target.find(['?', '#']).unwrap_or(target.len());
    let (path, tail) = target.split_at(split);
    let count = path.bytes().filter(|b| *b == b'/').count() + 1;
    let mut parts: Vec<&str> = Vec::new();
    parts
        .try_reserve_exact(count)
        .map_err(|_| Error::NoMemory)?;
    for (i, segment) in path.split('/').enumerate() {
        let last = i + 1 == count;
        match segment {
            "." => {
                if last {
                    parts.push("");
                }
            }
            ".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
                if last {
                    parts.push("");
                }
            }
            segment => parts.push(segment),
        }
    }
    let mut out = Out(String::new());
    out.0
        .try_reserve_exact(path.len() + tail.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    // As the old join: a path that does not start with a slash gets one.
    if !(parts.len() > 1 && parts.first().is_some_and(|first| first.is_empty())) {
        out.ch('/')?;
    }
    for (n, part) in parts.iter().enumerate() {
        if n > 0 {
            out.ch('/')?;
        }
        out.text(part)?;
    }
    out.text(tail)?;
    Ok(out.0)
}
