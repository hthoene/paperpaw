//! Host side of the service `core.secrets` (`docs/abi/secrets.md` 2 and 3).
//!
//! Five ops over the secrets file ([`super::secrets_file`]): `get`, `list`,
//! `keep`, `remove` and `random`. The backend is stateless; the shell wires a
//! [`SecretsHost`] (the card or the private folder, the [`Sealer`], the random
//! source) once. Until then the family answers `ERR_UNSUPPORTED` (the X4
//! profile wires nothing yet).
//!
//! The owner of a secret is the calling plugin's id, stamped by the host.
//! Another plugin's secrets are reachable only with the admitted
//! `uses.secrets`, and never their values. Values are never held in a log or a
//! `Debug`; the buffers that held one are wiped.
//!
//! `get` and `list` read the file line by line. `keep` and `remove` read it
//! once to learn what they need (a probe), then stream the old lines into a
//! new file with the one change and replace the old file by rename: a power cut
//! leaves the old file or the new, never half of either. Memory per call: the
//! read buffer (one line and a little room), two 1 KiB value buffers, and for
//! `keep` and `remove` one line besides; no allocation per secret. A line the host cannot open (another device's sealed value) is
//! absent for every op and written back unchanged by every rewrite.
//!
//! Order: the order of the lines in the file, so a page is streamed without
//! sorting or holding the file: an interface-6 file keeps its order, `keep`
//! appends a new secret and a changed one keeps its line. The `list` cursor is
//! a row number among the rows the caller may see.
use super::{
    host_services::Caller,
    registrations::{self as wire, Record},
    secrets_file::{
        self as file, Buf, Flow, LINE_MAX, NAME_MAX, Opener, Piece, READ_BUF, Scratch, VALUE_MAX,
        scan,
    },
};
use crate::abi7::{context::Frame, services_wire::ServiceOp};
use crate::native_host::codec;
use alloc::rc::Rc;
use core::cell::Cell;
use paperpaw_native::host_glue::{
    ERR_DENIED, ERR_FULL, ERR_INVALID, ERR_IO, ERR_NO_SPACE, ERR_NOT_FOUND, ERR_STATE,
    ERR_UNSUPPORTED,
};
use paperpaw_platform::Storage;

pub(crate) use super::secrets_file::Sealer;

/// `uses.secrets`: the feature that reaches another plugin's secrets.
const USES_SECRETS: &str = "uses.secrets";
/// Most secrets one plugin keeps.
pub(super) const PER_PLUGIN_MAX: u32 = 32;
/// `changeable`, in the flags of every secret record.
const CHANGEABLE: u32 = 1;
/// `all`, in the flags of `sec_list`.
const ALL: u32 = 1;
/// `strong`, in the flags of `sec_bytes`.
const STRONG: u32 = 1;
/// Largest `random` draw.
const RANDOM_MAX: usize = 256;

/// The device's random source.
///
/// Duties: `strong` is true only when the bytes come from a source that is
/// truly random now. On the ESP32-C3 the hardware RNG is only truly random
/// while the radio or the bootloader's entropy source is on, so a shell
/// reports `strong == false` (or fails) at other times; nothing that makes a
/// key, a token or a nonce may rely on weak bytes.
pub trait Rng {
    /// Fills `out` and says whether the source is fully random now (`strong`).
    /// A device without a source is not wired; there is no weaker stand-in.
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32>;
}

/// What the shell provides under the key `core.secrets`.
pub(crate) trait SecretsHost {
    /// Where the file lives: the card, or the app's private folder.
    fn storage(&self) -> &dyn Storage;
    /// How values are kept (decision 138).
    fn sealer(&self) -> &dyn Sealer;
    /// The random source, if the device has one.
    fn rng(&self) -> Option<&dyn Rng>;
    /// The file could not be read: show the user a notice (called once).
    fn unreadable(&self) {}
}

/// The view wired under `core.secrets`.
pub(crate) struct SecretsServices {
    pub(crate) host: Rc<dyn SecretsHost>,
    told: Cell<bool>,
}

impl SecretsServices {
    pub(crate) fn new(host: Rc<dyn SecretsHost>) -> Self {
        Self {
            host,
            told: Cell::new(false),
        }
    }

    /// Notes a failed read of the file (the notice is shown once) and passes
    /// the status on.
    fn failed(&self, status: i32) -> i32 {
        if status == ERR_IO && !self.told.replace(true) {
            self.host.unreadable();
        }
        status
    }

    /// First start after an update on a device that seals: rewrites interface
    /// 6's veiled values sealed. `Ok(true)` when the file was rewritten.
    ///
    /// # Errors
    /// `ERR_IO` (nothing changed), `ERR_NO_MEMORY`.
    pub(crate) fn upgrade(&self) -> Result<bool, i32> {
        if !self.host.sealer().seals() {
            return Ok(false);
        }
        let mut file = File::new(&*self.host)?;
        let probe = file.probe("", "").map_err(|e| self.failed(e))?;
        if !probe.veiled {
            return Ok(false);
        }
        file.rewrite(&Change::Upgrade)?;
        Ok(true)
    }

    /// Removes every secret of plugin `plugin` (its data was deleted by the
    /// user, decision 124), also those that do not open here.
    ///
    /// # Errors
    /// `ERR_IO` (nothing changed), `ERR_NO_MEMORY`, `ERR_INVALID`.
    #[allow(
        dead_code,
        reason = "the removal job uses [`forget_owner`]; tests this"
    )]
    pub(crate) fn forget_plugin(&self, plugin: &str) -> Result<(), i32> {
        forget_owner(&*self.host, plugin).map_err(|e| self.failed(e))
    }
}

/// Removes every secret of `plugin` from the file `host` keeps (the removal of
/// a plugin with its data, decision 124); safe to repeat.
///
/// # Errors
/// `ERR_IO` (nothing changed), `ERR_NO_MEMORY`, `ERR_INVALID`.
pub(crate) fn forget_owner(host: &dyn SecretsHost, plugin: &str) -> Result<(), i32> {
    check_owner(plugin)?;
    let mut file = File::new(host)?;
    file.rewrite(&Change::DropOwner(plugin))
}

/// The `core.secrets` backend.
pub(crate) fn backend(
    _frame: &dyn Frame,
    who: &Caller<'_>,
    op: &ServiceOp,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let secrets = who.hosts.secrets().ok_or(ERR_UNSUPPORTED)?;
    if op.id != 5 {
        check_owner(who.plugin)?;
    }
    match op.id {
        1 => get(secrets, who, input, out),
        2 => list(secrets, who, input, out),
        3 => keep(secrets, who, input).map(|()| 0),
        4 => remove(secrets, who, input).map(|()| 0),
        5 => random(secrets, input, out),
        _ => Err(ERR_UNSUPPORTED),
    }
}

/// A plugin id is an owner of the file when `.` (which ends the owner in a
/// line) and control characters are not in it.
fn check_owner(owner: &str) -> Result<(), i32> {
    let fine = !owner.is_empty()
        && owner.len() <= NAME_MAX
        && !owner.contains('.')
        && !owner.chars().any(char::is_control);
    if fine { Ok(()) } else { Err(ERR_INVALID) }
}

/// A name is 1-64 bytes without control characters (tab, line break and
/// carriage return are allowed: an SSID may hold them and the file escapes
/// them).
fn check_name(name: &str) -> Result<(), i32> {
    if name.is_empty() {
        return Err(ERR_INVALID);
    }
    check_label(name)
}

/// A name or a label: at most 64 bytes without control characters (C0 except
/// tab, line break and carriage return, DEL, C1).
fn check_label(text: &str) -> Result<(), i32> {
    let fine = text.len() <= NAME_MAX
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'));
    if fine { Ok(()) } else { Err(ERR_INVALID) }
}

/// Whose secret a call names: the caller's own, or another plugin's (which
/// needs `uses.secrets`).
fn target<'a>(who: &Caller<'a>, plugin: &'a str) -> Result<(&'a str, bool), i32> {
    if plugin.is_empty() || plugin == who.plugin {
        return Ok((who.plugin, true));
    }
    who.features.feature(USES_SECRETS)?;
    check_owner(plugin)?;
    Ok((plugin, false))
}

fn get(
    secrets: &SecretsServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let name: wire::ServiceSecName<'_> = codec::single(input)?;
    // Values never leave their owner, also not for the settings editor, and
    // `plugin` must be "" (secrets.md 2.1): an explicit id, also the caller's
    // own, is `ERR_DENIED`.
    if !name.plugin.is_empty() {
        return Err(ERR_DENIED);
    }
    check_name(name.name)?;
    let mut file = File::new(&*secrets.host)?;
    let mut found: Option<(bool, [u8; NAME_MAX], usize)> = None;
    let File {
        read,
        scratch,
        opener,
        ..
    } = &mut file;
    scan(secrets.host.storage(), file::FILE, read, &mut |piece| {
        let Piece::Line(line) = piece else {
            return Ok(Flow::Go);
        };
        let Some(row) = file::parse(line, scratch) else {
            return Ok(Flow::Go);
        };
        if row.owner != who.plugin || row.name != name.name || !opener.open(&row) {
            return Ok(Flow::Go);
        }
        let mut label = [0_u8; NAME_MAX];
        let label_len = row.label.len().min(NAME_MAX);
        let cut = (0..=label_len)
            .rev()
            .find(|at| row.label.is_char_boundary(*at))
            .unwrap_or(0);
        label[..cut].copy_from_slice(&row.label.as_bytes()[..cut]);
        found = Some((row.changeable, label, cut));
        Ok(Flow::Stop)
    })
    .map_err(|e| secrets.failed(e))?;
    let (changeable, label, label_len) = found.ok_or(ERR_NOT_FOUND)?;
    let label = core::str::from_utf8(&label[..label_len]).map_err(|_| ERR_IO)?;
    let record = Record::ServiceSecValue(wire::ServiceSecValue {
        flags: if changeable { CHANGEABLE } else { 0 },
        label,
        value: file.opener.value(),
    });
    codec::encode_one(&record, out)
}

fn list(
    secrets: &SecretsServices,
    who: &Caller<'_>,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let ask: wire::ServiceSecList = codec::single(input)?;
    if ask.flags & !ALL != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    let all = ask.flags & ALL != 0;
    if all {
        who.features.feature(USES_SECRETS)?;
    }
    let end = wire::ServiceEnd {
        flags: 0,
        total: 0,
        generation: 0,
        next: 0,
        cursor: None,
    };
    let reserved = codec::framed_len(&Record::ServiceEnd(end))?;
    let available = out.len().checked_sub(reserved).ok_or(ERR_NO_SPACE)?;
    let mut file = File::new(&*secrets.host)?;
    let File {
        read,
        scratch,
        opener,
        ..
    } = &mut file;
    let (mut used, mut rows, mut seen, mut more) = (0_usize, 0_u32, 0_u32, false);
    let mut failure = None;
    scan(secrets.host.storage(), file::FILE, read, &mut |piece| {
        let Piece::Line(line) = piece else {
            return Ok(Flow::Go);
        };
        let Some(row) = file::parse(line, scratch) else {
            return Ok(Flow::Go);
        };
        if (!all && row.owner != who.plugin) || !opener.open(&row) {
            return Ok(Flow::Go);
        }
        opener.plain.clear();
        seen += 1;
        if seen <= ask.cursor || more {
            // Before the page, or after it: only counted.
            return Ok(Flow::Go);
        }
        let record = Record::ServiceSecEntry(wire::ServiceSecEntry {
            flags: if row.changeable { CHANGEABLE } else { 0 },
            plugin: row.owner,
            name: row.name,
            label: row.label,
        });
        let room = out.get_mut(used..available).unwrap_or_default();
        let written = if rows as usize == crate::abi7::services_wire::core_secrets_v1::PAGE_MAX_ROWS
        {
            None
        } else {
            codec::encode_one(&record, room).ok()
        };
        match written {
            Some(length) => {
                used += length;
                rows += 1;
                Ok(Flow::Go)
            }
            None => {
                more = true;
                if rows == 0 {
                    failure = Some(ERR_NO_SPACE);
                    return Ok(Flow::Stop);
                }
                Ok(Flow::Go)
            }
        }
    })
    .map_err(|e| secrets.failed(e))?;
    if let Some(error) = failure {
        return Err(error);
    }
    let end = wire::ServiceEnd {
        flags: u32::from(!more),
        total: seen,
        next: ask.cursor.checked_add(rows).ok_or(ERR_INVALID)?,
        ..end
    };
    let length = codec::encode_one(&Record::ServiceEnd(end), &mut out[used..])?;
    Ok(used + length)
}

fn keep(secrets: &SecretsServices, who: &Caller<'_>, input: &[u8]) -> Result<(), i32> {
    let ask: wire::ServiceSecKeep<'_> = codec::single(input)?;
    if ask.flags & !CHANGEABLE != 0 {
        return Err(ERR_UNSUPPORTED);
    }
    if ask.value.len() > VALUE_MAX || ask.label.len() > NAME_MAX {
        return Err(ERR_INVALID);
    }
    check_name(ask.name)?;
    check_label(ask.label)?;
    // A device that does not seal writes nothing (decision 140): no file is
    // opened, no temporary file is made.
    if !secrets.host.sealer().seals() {
        return Err(ERR_STATE);
    }
    let (owner, own) = target(who, ask.plugin)?;
    let mut file = File::new(&*secrets.host)?;
    let probe = file.probe(owner, ask.name).map_err(|e| secrets.failed(e))?;
    if !own && !probe.readable {
        return Err(ERR_NOT_FOUND);
    }
    if !own && !probe.changeable {
        return Err(ERR_DENIED);
    }
    if !probe.readable && probe.others >= PER_PLUGIN_MAX {
        return Err(ERR_FULL);
    }
    let kept_label = probe.label_str();
    let label = if ask.label.is_empty() && probe.readable {
        kept_label
    } else {
        ask.label
    };
    // The editor cannot change `changeable`.
    let changeable = if own {
        ask.flags & CHANGEABLE != 0
    } else {
        probe.changeable
    };
    let change = Change::Put {
        owner,
        name: ask.name,
        label,
        changeable,
        value: ask.value.as_bytes(),
    };
    file.rewrite(&change).map_err(|e| secrets.failed(e))
}

fn remove(secrets: &SecretsServices, who: &Caller<'_>, input: &[u8]) -> Result<(), i32> {
    let ask: wire::ServiceSecName<'_> = codec::single(input)?;
    check_name(ask.name)?;
    let (owner, _) = target(who, ask.plugin)?;
    let mut file = File::new(&*secrets.host)?;
    let probe = file.probe(owner, ask.name).map_err(|e| secrets.failed(e))?;
    if !probe.readable {
        // None here, or one that does not open (kept as it is).
        return Ok(());
    }
    file.rewrite(&Change::Remove {
        owner,
        name: ask.name,
    })
    .map_err(|e| secrets.failed(e))
}

fn random(secrets: &SecretsServices, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
    let ask: wire::ServiceSecRandom = codec::single(input)?;
    let len = usize::try_from(ask.len).map_err(|_| ERR_INVALID)?;
    if len == 0 || len > RANDOM_MAX {
        return Err(ERR_INVALID);
    }
    let rng = secrets.host.rng().ok_or(ERR_UNSUPPORTED)?;
    let mut bytes = [0_u8; RANDOM_MAX];
    let drawn = rng.fill(&mut bytes[..len]);
    let result = drawn.and_then(|strong| {
        let record = Record::ServiceSecBytes(wire::ServiceSecBytes {
            flags: if strong { STRONG } else { 0 },
            data: &bytes[..len],
        });
        codec::encode_one(&record, out)
    });
    file::wipe(&mut bytes);
    result
}

/// What a probe of the file learns about one name.
struct Probe {
    /// Some line has this owner and name (readable here or not).
    any: bool,
    /// One of them opens.
    readable: bool,
    changeable: bool,
    label: [u8; NAME_MAX],
    label_len: usize,
    /// Other names of this owner that open here.
    others: u32,
    /// A veiled value that opens is in the file.
    veiled: bool,
}

impl Probe {
    fn label_str(&self) -> &str {
        core::str::from_utf8(&self.label[..self.label_len]).unwrap_or_default()
    }
}

/// The one change a rewrite makes.
enum Change<'a> {
    /// Only veiled values become sealed.
    Upgrade,
    Put {
        owner: &'a str,
        name: &'a str,
        label: &'a str,
        changeable: bool,
        value: &'a [u8],
    },
    Remove {
        owner: &'a str,
        name: &'a str,
    },
    DropOwner(&'a str),
}

/// The buffers of one call. The line a rewrite builds is not among them: it
/// is made by [`File::rewrite`] only, so that `get` and `list` (the settings
/// screen's calls) do not hold it.
struct File<'a> {
    host: &'a dyn SecretsHost,
    read: Buf,
    scratch: Scratch,
    opener: Opener<'a>,
}

impl<'a> File<'a> {
    fn new(host: &'a dyn SecretsHost) -> Result<Self, i32> {
        Ok(Self {
            host,
            read: Buf::new(READ_BUF)?,
            scratch: Scratch::new()?,
            opener: Opener::new(host.sealer())?,
        })
    }

    fn probe(&mut self, owner: &str, name: &str) -> Result<Probe, i32> {
        let mut probe = Probe {
            any: false,
            readable: false,
            changeable: false,
            label: [0; NAME_MAX],
            label_len: 0,
            others: 0,
            veiled: false,
        };
        let Self {
            host,
            read,
            scratch,
            opener,
            ..
        } = self;
        scan(host.storage(), file::FILE, read, &mut |piece| {
            let Piece::Line(line) = piece else {
                return Ok(Flow::Go);
            };
            let Some(row) = file::parse(line, scratch) else {
                return Ok(Flow::Go);
            };
            let veiled = matches!(row.field, file::Field::Veiled(_));
            let same = row.owner == owner && row.name == name;
            if same {
                probe.any = true;
            }
            let wanted = (same && !probe.readable) || (!same && row.owner == owner) || veiled;
            let opens = wanted && opener.open(&row);
            probe.veiled |= opens && veiled;
            if opens && same && !probe.readable {
                probe.readable = true;
                probe.changeable = row.changeable;
                let cut = (0..=row.label.len().min(NAME_MAX))
                    .rev()
                    .find(|at| row.label.is_char_boundary(*at))
                    .unwrap_or(0);
                probe.label[..cut].copy_from_slice(&row.label.as_bytes()[..cut]);
                probe.label_len = cut;
            } else if opens && !same && row.owner == owner {
                probe.others += 1;
            }
            opener.plain.clear();
            Ok(Flow::Go)
        })?;
        Ok(probe)
    }

    /// Streams the old file with `change` into the new file and replaces the
    /// old by rename. On any failure the new file is removed and the old stays.
    fn rewrite(&mut self, change: &Change<'_>) -> Result<(), i32> {
        let mut line = Buf::new(LINE_MAX + 1)?;
        let storage = self.host.storage();
        storage.write(file::TEMP, b"").map_err(file::status)?;
        let result = self.stream(change, &mut line).and_then(|written| {
            let old = storage.size(file::FILE).unwrap_or(0);
            if matches!(change, Change::Put { .. }) && written > file::FILE_MAX && written > old {
                return Err(ERR_FULL);
            }
            storage.sync().map_err(file::status)?;
            storage
                .rename(file::TEMP, file::FILE)
                .map_err(file::status)?;
            storage.sync().map_err(file::status)
        });
        if result.is_err() {
            let _ = storage.remove(file::TEMP);
        }
        result
    }

    fn stream(&mut self, change: &Change<'_>, line: &mut Buf) -> Result<u64, i32> {
        let Self {
            host,
            read,
            scratch,
            opener,
        } = self;
        let storage = host.storage();
        let sealer = host.sealer();
        let mut written = 0_u64;
        let mut placed = false;
        let put = |bytes: &[u8], written: &mut u64| -> Result<(), i32> {
            storage.append(file::TEMP, bytes).map_err(file::status)?;
            *written += bytes.len() as u64;
            Ok(())
        };
        scan(storage, file::FILE, read, &mut |piece| {
            let line_bytes = match piece {
                Piece::Opaque { bytes, end } => {
                    put(bytes, &mut written)?;
                    if end {
                        put(b"\n", &mut written)?;
                    }
                    return Ok(Flow::Go);
                }
                Piece::Line(bytes) => bytes,
            };
            let keep_as_is = |written: &mut u64| -> Result<(), i32> {
                put(line_bytes, written)?;
                if !line_bytes.ends_with(b"\n") {
                    put(b"\n", written)?;
                }
                Ok(())
            };
            let Some(row) = file::parse(line_bytes, scratch) else {
                keep_as_is(&mut written)?;
                return Ok(Flow::Go);
            };
            let (place_first, drop_row) = decide(change, &row, &mut placed, opener);
            if place_first {
                put_new(change, line, opener, sealer)?;
                put(line.as_bytes(), &mut written)?;
                line.clear();
            }
            if drop_row {
                return Ok(Flow::Go);
            }
            let reseal =
                sealer.seals() && matches!(row.field, file::Field::Veiled(_)) && opener.open(&row);
            if reseal {
                let row_parts = (row.owner, row.name, row.label, row.changeable);
                let Opener { plain, blob, .. } = &mut *opener;
                file::build(line, blob, sealer, row_parts, plain.as_bytes())?;
                put(line.as_bytes(), &mut written)?;
                line.clear();
            } else {
                keep_as_is(&mut written)?;
            }
            opener.plain.clear();
            Ok(Flow::Go)
        })?;
        if !placed && matches!(change, Change::Put { .. }) {
            put_new(change, line, opener, sealer)?;
            put(line.as_bytes(), &mut written)?;
            line.clear();
        }
        Ok(written)
    }
}

/// What `change` does with `row`: whether the new line goes before it, and
/// whether the row itself goes. The rest is kept (a veiled value that opens is
/// sealed on a device that seals).
fn decide(
    change: &Change<'_>,
    row: &file::Row<'_>,
    placed: &mut bool,
    opener: &mut Opener<'_>,
) -> (bool, bool) {
    use core::cmp::Ordering;
    match change {
        Change::Put { owner, name, .. } => match (row.owner, row.name).cmp(&(*owner, *name)) {
            Ordering::Equal if *placed => (false, opener.open(row)),
            Ordering::Equal => {
                *placed = true;
                (true, true)
            }
            _ => (false, false),
        },
        Change::Remove { owner, name } => (
            false,
            row.owner == *owner && row.name == *name && opener.open(row),
        ),
        Change::DropOwner(owner) => (false, row.owner == *owner),
        Change::Upgrade => (false, false),
    }
}

/// The new line of a `Put` in `line`, or nothing for another change.
fn put_new(
    change: &Change<'_>,
    line: &mut Buf,
    opener: &mut Opener<'_>,
    sealer: &dyn Sealer,
) -> Result<(), i32> {
    line.clear();
    if let Change::Put {
        owner,
        name,
        label,
        changeable,
        value,
    } = change
    {
        file::build(
            line,
            &mut opener.blob,
            sealer,
            (owner, name, label, *changeable),
            value,
        )?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "secrets_service_tests.rs"]
pub(crate) mod tests;
