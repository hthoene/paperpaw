//! `core.secrets`: owner stamping, the limits, the statuses, sealing and the
//! upgrade, lines the host cannot open, streaming under a small heap, refusals
//! by storage and by memory.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::abi7::services_wire;
use crate::native_host::admitted_features::AdmittedFeatures;
use crate::native_host::call_services::validate_reply;
use alloc::{string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_native::metadata::Feature;
use paperpaw_platform::{DirEntry, Instant, StorageError};
use paperpaw_sdk::{Env, Settings};
use std::collections::BTreeMap;

static WIRED: &[Feature<'static>] = &[Feature {
    key: "uses.secrets",
    revision: 1,
}];
const NONE: AdmittedFeatures = AdmittedFeatures::NONE;

fn editor() -> AdmittedFeatures {
    AdmittedFeatures::test(1, WIRED)
}

/// A card in memory that can be made to fail, and that remembers its largest
/// read.
#[derive(Default)]
struct Card {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    fail_read: Cell<bool>,
    fail_rename: Cell<bool>,
    /// Appends that still work before the next fails (`u32::MAX`: all).
    appends_left: Cell<u32>,
    largest_read: Cell<usize>,
    renames: Cell<u32>,
}

impl Card {
    fn new() -> Self {
        let card = Self::default();
        card.appends_left.set(u32::MAX);
        card
    }
    fn put(&self, path: &str, data: &[u8]) {
        self.files.borrow_mut().insert(path.into(), data.to_vec());
    }
    fn get(&self, path: &str) -> Option<Vec<u8>> {
        self.files.borrow().get(path).cloned()
    }
    fn file(&self) -> Vec<u8> {
        self.get(file::FILE).unwrap_or_default()
    }
}

/// The card's own allocations are the fixture's, not the host's: the failure
/// injection waits while the card works.
struct Shield(u32, bool);
impl Shield {
    fn new() -> Self {
        let nth = super::super::shell::tests::FAIL_NTH.with(|n| n.replace(0));
        let shielded = super::super::shell::tests::HEAP_SHIELD.with(|s| s.replace(true));
        Self(nth, shielded)
    }
}
impl Drop for Shield {
    fn drop(&mut self) {
        super::super::shell::tests::FAIL_NTH.with(|n| n.set(self.0));
        super::super::shell::tests::HEAP_SHIELD.with(|s| s.set(self.1));
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let _shield = Shield::new();
        if self.fail_read.get() {
            return Err(StorageError::Io);
        }
        self.largest_read
            .set(self.largest_read.get().max(buf.len()));
        let files = self.files.borrow();
        let data = files.get(path).ok_or(StorageError::NotFound)?;
        let start = usize::try_from(offset).unwrap().min(data.len());
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.files
            .borrow()
            .get(path)
            .map(|d| d.len() as u64)
            .ok_or(StorageError::NotFound)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let _shield = Shield::new();
        self.put(path, data);
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        let _shield = Shield::new();
        let left = self.appends_left.get();
        if left == 0 {
            return Err(StorageError::Io);
        }
        self.appends_left.set(left.saturating_sub(1));
        self.files
            .borrow_mut()
            .entry(path.into())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        let _shield = Shield::new();
        self.files.borrow_mut().remove(path);
        Ok(())
    }
    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        let _shield = Shield::new();
        if self.fail_rename.get() {
            return Err(StorageError::Io);
        }
        self.renames.set(self.renames.get() + 1);
        let data = self
            .files
            .borrow_mut()
            .remove(from)
            .ok_or(StorageError::NotFound)?;
        self.files.borrow_mut().insert(to.into(), data);
        Ok(())
    }
}

/// Seals with a key: the key mixes into the value, the tag covers the line's
/// owner and name and the sealed bytes (a stand-in for AES-256-GCM).
struct Seal {
    key: u8,
    seals: bool,
    nonce: Cell<u8>,
    /// The random numbers are weak now: a seal is `ERR_STATE` (the X4 with
    /// its radio off).
    weak: Rc<Cell<bool>>,
}

impl Seal {
    fn new(key: u8) -> Self {
        Self {
            key,
            seals: true,
            nonce: Cell::new(1),
            weak: Rc::new(Cell::new(false)),
        }
    }
    fn tag(&self, aad: &[u8], body: &[u8]) -> [u8; 16] {
        let mut tag = [self.key; 16];
        for (i, b) in aad.iter().chain(body).enumerate() {
            tag[i % 16] = tag[i % 16].rotate_left(3) ^ b ^ self.key;
        }
        tag
    }
}

impl Sealer for Seal {
    fn seals(&self) -> bool {
        self.seals
    }
    fn seal(&self, aad: &[u8], plain: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        if self.weak.get() {
            return Err(ERR_STATE);
        }
        let nonce = [self.nonce.replace(self.nonce.get().wrapping_add(1)); 12];
        let total = 12 + plain.len() + 16;
        let out = out.get_mut(..total).ok_or(ERR_NO_SPACE)?;
        out[..12].copy_from_slice(&nonce);
        for (i, b) in plain.iter().enumerate() {
            out[12 + i] = b ^ self.key ^ nonce[0];
        }
        let tag = self.tag(aad, &out[..12 + plain.len()]);
        out[12 + plain.len()..].copy_from_slice(&tag);
        Ok(total)
    }
    fn open(&self, aad: &[u8], sealed: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let body_len = sealed.len().checked_sub(28).ok_or(ERR_IO)?;
        let (body, tag) = sealed.split_at(12 + body_len);
        if self.tag(aad, body) != tag {
            return Err(ERR_IO);
        }
        let out = out.get_mut(..body_len).ok_or(ERR_NO_SPACE)?;
        for (i, b) in body[12..].iter().enumerate() {
            out[i] = b ^ self.key ^ body[0];
        }
        Ok(body_len)
    }
}

struct Source {
    strong: Cell<bool>,
}
impl Rng for Source {
    fn fill(&self, out: &mut [u8]) -> Result<bool, i32> {
        for (i, b) in out.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(7).wrapping_add(3);
        }
        Ok(self.strong.get())
    }
}

struct Host {
    card: Card,
    seal: Seal,
    source: Source,
    has_source: bool,
    notices: Cell<u32>,
}
impl SecretsHost for Host {
    fn storage(&self) -> &dyn Storage {
        &self.card
    }
    fn sealer(&self) -> &dyn Sealer {
        &self.seal
    }
    fn rng(&self) -> Option<&dyn Rng> {
        self.has_source.then_some(&self.source as &dyn Rng)
    }
    fn unreadable(&self) {
        self.notices.set(self.notices.get() + 1);
    }
}

/// The rows (owner, name, label, flags), `next`, `last`.
type Page = (Vec<(String, String, String, u32)>, u32, bool);

struct View<'a> {
    env: &'a Env<'a>,
    hosts: crate::native_host::host_services::HostServices,
    host: Rc<Host>,
    /// The nth allocation inside the backend fails (0: none).
    fail: Cell<u32>,
    /// The most the last call held beyond what it started with.
    held: Cell<usize>,
    /// The `total` of the last list's `end`.
    total: Cell<u32>,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
}

impl View<'_> {
    fn call(
        &self,
        plugin: &str,
        features: AdmittedFeatures,
        op: u32,
        record: Record<'_>,
    ) -> Result<Vec<u8>, i32> {
        let schema = services_wire::service_schema("core.secrets", 1).unwrap();
        let op = schema.op(op).unwrap();
        let mut input = std::vec![0; 4096];
        let used = wire::records_encode_counted(&[record], 1, op.max_input, &mut input)?;
        self.call_raw(plugin, features, op, &input[..used])
    }
    fn call_raw(
        &self,
        plugin: &str,
        features: AdmittedFeatures,
        op: &ServiceOp,
        input: &[u8],
    ) -> Result<Vec<u8>, i32> {
        let caller = Caller {
            owner: 1,
            plugin,
            features,
            hosts: &self.hosts,
        };
        let mut out = std::vec![0; op.max_reply];
        super::super::shell::tests::FAIL_NTH.with(|n| n.set(self.fail.get()));
        let base = super::super::shell::tests::HEAP_LIVE.with(Cell::get);
        super::super::shell::tests::HEAP_PEAK.with(|p| p.set(base));
        let result = backend(self, &caller, op, input, &mut out);
        let peak = super::super::shell::tests::HEAP_PEAK.with(Cell::get);
        self.held.set(peak.saturating_sub(base));
        super::super::shell::tests::FAIL_NTH.with(|n| n.set(0));
        let length = result?;
        out.truncate(length);
        validate_reply(&out, op)?;
        Ok(out)
    }
    fn keep(&self, plugin: &str, name: &str, label: &str, value: &str) -> Result<(), i32> {
        self.keep_flags(plugin, "", name, label, value, 0)
    }
    fn keep_flags(
        &self,
        plugin: &str,
        target: &str,
        name: &str,
        label: &str,
        value: &str,
        flags: u32,
    ) -> Result<(), i32> {
        let features = if target.is_empty() { NONE } else { editor() };
        self.call(
            plugin,
            features,
            3,
            Record::ServiceSecKeep(wire::ServiceSecKeep {
                plugin: target,
                name,
                label,
                flags,
                value,
            }),
        )
        .map(|_| ())
    }
    fn get(&self, plugin: &str, name: &str) -> Result<(String, String, u32), i32> {
        let reply = self.call(
            plugin,
            NONE,
            1,
            Record::ServiceSecName(wire::ServiceSecName { plugin: "", name }),
        )?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceSecValue(v) => Ok((v.value.into(), v.label.into(), v.flags)),
            other => panic!("{other:?}"),
        }
    }
    fn remove(&self, plugin: &str, target: &str, name: &str) -> Result<(), i32> {
        let features = if target.is_empty() { NONE } else { editor() };
        self.call(
            plugin,
            features,
            4,
            Record::ServiceSecName(wire::ServiceSecName {
                plugin: target,
                name,
            }),
        )
        .map(|_| ())
    }
    /// One page: the rows (owner, name, label, flags), `next`, `last`.
    fn list(
        &self,
        plugin: &str,
        features: AdmittedFeatures,
        all: bool,
        cursor: u32,
    ) -> Result<Page, i32> {
        let reply = self.call(
            plugin,
            features,
            2,
            Record::ServiceSecList(wire::ServiceSecList {
                flags: u32::from(all),
                cursor,
            }),
        )?;
        let (mut rows, mut end) = (Vec::new(), None);
        crate::native_host::stage_wire::visit_stream(&reply, |_, framed| {
            match wire::record_decode_one(framed).unwrap() {
                Record::ServiceSecEntry(e) => {
                    rows.push((e.plugin.into(), e.name.into(), e.label.into(), e.flags));
                }
                Record::ServiceEnd(e) => end = Some((e.next, e.flags & 1 == 1, e.total)),
                other => panic!("{other:?}"),
            }
            Ok(())
        })
        .unwrap();
        let (next, last, total) = end.unwrap();
        self.total.set(total);
        Ok((rows, next, last))
    }
    fn names(&self, plugin: &str) -> Vec<String> {
        let mut names = Vec::new();
        let mut cursor = 0;
        loop {
            let (rows, next, last) = self.list(plugin, NONE, false, cursor).unwrap();
            names.extend(rows.into_iter().map(|r| r.1));
            if last {
                return names;
            }
            cursor = next;
        }
    }
    fn services(&self) -> &SecretsServices {
        self.hosts.secrets().unwrap()
    }
}

fn host(seal: Seal) -> Rc<Host> {
    Rc::new(Host {
        card: Card::new(),
        seal,
        source: Source {
            strong: Cell::new(true),
        },
        has_source: true,
        notices: Cell::new(0),
    })
}

fn with_host<R>(host: Rc<Host>, wire_it: bool, f: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    if wire_it {
        hosts
            .wire_secrets(SecretsServices::new(host.clone()))
            .unwrap();
    }
    f(&View {
        env: &env,
        hosts,
        host,
        fail: Cell::new(0),
        held: Cell::new(0),
        total: Cell::new(0),
    })
}

fn with_view<R>(f: impl FnOnce(&View<'_>) -> R) -> R {
    with_host(host(Seal::new(0x5a)), true, f)
}

/// A file as interface 6 wrote it: veiled values.
fn interface6(lines: &[(&str, &str, bool, &str)]) -> Vec<u8> {
    let mut secrets = paperpaw_sdk::secrets::Secrets::default();
    for (id, label, changeable, value) in lines {
        secrets.apply(
            paperpaw_sdk::secrets::SecretChange::keep(id, label, value).changeable(*changeable),
        );
    }
    secrets.encode().into_bytes()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn an_unwired_family_is_unsupported_and_a_dotted_caller_is_refused() {
    with_host(host(Seal::new(1)), false, |v| {
        assert_eq!(
            v.keep("mail", "password", "", "x").err(),
            Some(ERR_UNSUPPORTED)
        );
    });
    with_view(|v| {
        assert_eq!(v.keep("core.mail", "p", "", "x").err(), Some(ERR_INVALID));
        assert_eq!(v.get("", "p").err(), Some(ERR_INVALID));
    });
}

#[test]
fn a_kept_secret_is_the_callers_own_sealed_on_the_card_and_read_back() {
    with_view(|v| {
        v.keep("mail", "password", "mail.password", "hunter2")
            .unwrap();
        assert_eq!(
            v.get("mail", "password").unwrap(),
            ("hunter2".into(), "mail.password".into(), 0)
        );
        // Owner stamped: another plugin's `password` is another secret.
        assert_eq!(v.get("wifi", "password").err(), Some(ERR_NOT_FOUND));
        let file = text(&v.host.card.file());
        assert!(
            file.starts_with("mail.password\tmail.password\t\ts1:"),
            "{file}"
        );
        assert!(!file.contains("hunter2"));
        assert!(!file.contains("68756e74657232"), "not even hexed");
        assert!(v.host.card.get(file::TEMP).is_none());
        // Replace keeps the label when "", and the flag follows the owner.
        v.keep_flags("mail", "", "password", "", "second", 1)
            .unwrap();
        assert_eq!(
            v.get("mail", "password").unwrap(),
            ("second".into(), "mail.password".into(), 1)
        );
        assert_eq!(text(&v.host.card.file()).lines().count(), 1);
        // An empty value is a value.
        v.keep("mail", "open", "", "").unwrap();
        assert_eq!(v.get("mail", "open").unwrap().0, "");
    });
}

#[test]
fn the_limits_are_32_per_plugin_names_of_64_values_of_1024_and_a_64_kib_file() {
    with_view(|v| {
        for n in 0..32 {
            v.keep("mail", &std::format!("n{n:02}"), "", "v").unwrap();
        }
        assert_eq!(v.keep("mail", "n32", "", "v").err(), Some(ERR_FULL));
        // Replacing one at the limit works; another plugin has its own 32.
        v.keep("mail", "n05", "", "again").unwrap();
        v.keep("wifi", "n32", "", "v").unwrap();
        // Name: 64 bytes fit, 65 and empty and control characters do not.
        let long = "n".repeat(64);
        v.keep("wifi", &long, "", "v").unwrap();
        assert!(v.keep("wifi", &"n".repeat(65), "", "v").is_err());
        assert_eq!(v.keep("wifi", "", "", "v").err(), Some(ERR_INVALID));
        assert_eq!(v.keep("wifi", "a\u{7}b", "", "v").err(), Some(ERR_INVALID));
        // Value: 1,024 bytes fit (and come back), 1,025 do not.
        let big = "v".repeat(1_024);
        v.keep("wifi", "big", "", &big).unwrap();
        assert_eq!(v.get("wifi", "big").unwrap().0, big);
        assert!(v.keep("wifi", "bigger", "", &"v".repeat(1_025)).is_err());
        // The longest line (64+64 bytes of tabs in names, label, 1,024) fits.
        let tabs = "\t".repeat(64);
        v.keep("wifi", &tabs, &tabs, &big).unwrap();
        assert_eq!(v.get("wifi", &tabs).unwrap(), (big, tabs, 0));
        // Unknown flag bits are unsupported (a later host may know them).
        assert_eq!(
            v.keep_flags("wifi", "", "x", "", "v", 2).err(),
            Some(ERR_UNSUPPORTED)
        );
    });
}

#[test]
fn the_file_stops_at_64_kib_and_the_old_file_stays() {
    with_view(|v| {
        let big = "v".repeat(1_024);
        let mut count = 0;
        // Fill with 1 KiB values until the file is full.
        let full = loop {
            let owner = std::format!("p{}", count / 30);
            match v.keep(&owner, &std::format!("n{}", count % 30), "", &big) {
                Ok(()) => count += 1,
                Err(e) => break e,
            }
            assert!(count < 100);
        };
        assert_eq!(full, ERR_FULL);
        let size = v.host.card.file().len();
        assert!(size as u64 <= file::FILE_MAX && size > 60_000, "{size}");
        // A remove still works on a full file, and then there is room again.
        v.remove("p0", "", "n0").unwrap();
        v.keep("p0", "n0", "", &big).unwrap();
        assert!(v.host.card.get(file::TEMP).is_none());
    });
}

#[test]
fn names_and_labels_with_tabs_breaks_and_backslashes_come_back() {
    with_view(|v| {
        let name = "Home\tNet\\x\r\ny";
        v.keep("wifi", name, "la\tbel\\", "pw").unwrap();
        assert_eq!(
            v.get("wifi", name).unwrap(),
            ("pw".into(), "la\tbel\\".into(), 0)
        );
        assert_eq!(
            v.host.card.file().iter().filter(|b| **b == b'\n').count(),
            1
        );
        assert_eq!(v.names("wifi"), [name]);
    });
}

#[test]
fn lines_the_host_does_not_read_are_kept_byte_for_byte_by_every_rewrite() {
    let long = std::format!("mail.long\t\t\t{}\n", "ab".repeat(3_000));
    // Labels of 65 to 128 bytes and control characters in a name or label are
    // lines the API would refuse: kept, never listed (they must not wedge a
    // listing as a row that cannot be encoded).
    let wide = std::format!("mail.wide\t{}\t\t00\n", "L".repeat(65));
    let wider = std::format!("mail.wider\t{}\t\t00\n", "L".repeat(128));
    let odd: Vec<&[u8]> = std::vec![
        wide.as_bytes(),
        wider.as_bytes(),
        b"mail.a\x07b\t\t\t00\n",
        b"mail.ok\tla\x7fbel\t\t00\n",
        b"just words\n",
        b"mail.x\tlabel\tz\t00\n",
        b"mail.nohex\t\t\tnothex\n",
        b"mail.odd\t\t\tabc\n",
        b"\xff\xfe\t\t\t00\n",
        b".noowner\t\t\t00\n",
        b"mail.future\tl\tc\t00\textra\n",
        b"mail.\t\t\t00",
    ];
    let mut file = Vec::new();
    for line in &odd {
        file.extend_from_slice(line);
        if !line.ends_with(b"\n") {
            file.push(b'\n');
        }
    }
    file.extend_from_slice(long.as_bytes());
    with_view(|v| {
        v.host.card.put(file::FILE, &file);
        assert_eq!(v.names("mail"), Vec::<String>::new());
        v.keep("mail", "password", "", "pw").unwrap();
        v.keep("mail", "other", "", "pw2").unwrap();
        v.remove("mail", "", "password").unwrap();
        v.remove("mail", "", "gone").unwrap();
        let after = v.host.card.file();
        // Every unread line is still there, in order, unchanged.
        let mut rest = &after[..];
        for line in odd
            .iter()
            .map(|l| l.strip_suffix(b"\n").unwrap_or(l))
            .chain([long.trim_end().as_bytes()])
        {
            let at = rest
                .windows(line.len())
                .position(|w| w == line)
                .unwrap_or_else(|| panic!("{} lost", text(line)));
            rest = &rest[at + line.len()..];
        }
        assert_eq!(v.get("mail", "other").unwrap().0, "pw2");
        assert_eq!(v.get("mail", "password").err(), Some(ERR_NOT_FOUND));
    });
}

#[test]
fn a_value_from_another_device_is_absent_and_kept_until_the_user_enters_it_again() {
    // The card was written by a device with key 0x11.
    let theirs = host(Seal::new(0x11));
    let line = with_host(theirs, true, |v| {
        v.keep("mail", "password", "L", "foreign").unwrap();
        v.keep("mail", "token", "T", "foreign2").unwrap();
        v.host.card.file()
    });
    with_view(|v| {
        v.host.card.put(file::FILE, &line);
        v.keep("mail", "mine", "", "m").unwrap();
        assert_eq!(v.get("mail", "password").err(), Some(ERR_NOT_FOUND));
        assert_eq!(v.names("mail"), ["mine"]);
        assert_eq!(v.list("mail", NONE, false, 0).unwrap().0.len(), 1);
        // A keep and a remove of other names write the lines back unchanged.
        v.remove("mail", "", "mine").unwrap();
        v.keep("wifi", "net", "", "w").unwrap();
        let after = text(&v.host.card.file());
        for original in text(&line).lines() {
            assert!(after.lines().any(|l| l == original), "{original}");
        }
        // Removing the absent name is OK and changes nothing.
        let renames = v.host.card.renames.get();
        v.remove("mail", "", "password").unwrap();
        assert_eq!(v.host.card.renames.get(), renames);
        assert_eq!(text(&v.host.card.file()), after);
        // A keep of the same name replaces it: the way to enter it again.
        v.keep("mail", "password", "", "mine now").unwrap();
        assert_eq!(v.get("mail", "password").unwrap().0, "mine now");
        let after = text(&v.host.card.file());
        assert_eq!(after.matches("mail.password").count(), 1);
        assert!(after.contains("mail.token"));
    });
}

#[test]
fn a_value_copied_to_another_line_does_not_open_there() {
    with_view(|v| {
        v.keep("mail", "a", "", "secret-a").unwrap();
        let file = text(&v.host.card.file());
        let moved = file.replace("mail.a\t", "mail.b\t");
        v.host.card.put(file::FILE, moved.as_bytes());
        assert_eq!(v.get("mail", "b").err(), Some(ERR_NOT_FOUND));
    });
}

#[test]
fn interface_6_values_are_read_and_become_sealed_at_the_first_save() {
    let old = interface6(&[
        ("mail.password", "mail.password", false, "old-pw"),
        ("wifi.Home", "Home", true, "wifi-pw"),
    ]);
    with_view(|v| {
        v.host.card.put(file::FILE, &old);
        // Read as they are.
        assert_eq!(v.get("mail", "password").unwrap().0, "old-pw");
        assert_eq!(
            v.get("wifi", "Home").unwrap(),
            ("wifi-pw".into(), "Home".into(), 1)
        );
        // The first start after the update saves at once.
        assert_eq!(v.services().upgrade(), Ok(true));
        let file = text(&v.host.card.file());
        assert_eq!(file.matches("s1:").count(), 2, "{file}");
        assert!(
            !file.contains(
                &interface6(&[("mail.password", "mail.password", false, "old-pw")])
                    .iter()
                    .map(|b| *b as char)
                    .collect::<String>()
            )
        );
        assert_eq!(v.get("mail", "password").unwrap().0, "old-pw");
        assert_eq!(v.get("wifi", "Home").unwrap().2, 1);
        // Nothing left to upgrade.
        assert_eq!(v.services().upgrade(), Ok(false));
    });
    // A keep alone also seals what was veiled.
    with_view(|v| {
        v.host.card.put(file::FILE, &old);
        v.keep("stats", "webdav", "", "d").unwrap();
        assert_eq!(text(&v.host.card.file()).matches("s1:").count(), 3);
    });
}

#[test]
fn a_device_that_does_not_seal_refuses_keep_and_writes_nothing_but_still_reads_veiled_lines() {
    let old = interface6(&[
        ("mail.password", "mail.password", false, "old-pw"),
        ("wifi.Home", "wifi.Home", true, "wpw"),
    ]);
    let mut seal = Seal::new(9);
    seal.seals = false;
    with_host(host(seal), true, |v| {
        v.host.card.put(file::FILE, &old);
        // Veiled values are read.
        assert_eq!(v.get("mail", "password").unwrap().0, "old-pw");
        // A keep (new, or a changed one) is refused with nothing written: the
        // file is byte for byte what it was, and no temporary file is left.
        assert_eq!(
            v.keep("mail", "password", "L", "new").err(),
            Some(ERR_STATE)
        );
        assert_eq!(v.keep("stats", "webdav", "", "d").err(), Some(ERR_STATE));
        assert_eq!(v.host.card.file(), old);
        assert_eq!(v.host.card.renames.get(), 0);
        assert!(v.host.card.files.borrow().get(file::TEMP).is_none());
        // Nothing to upgrade where nothing seals: the veiled lines stay.
        assert_eq!(v.services().upgrade(), Ok(false));
        assert_eq!(v.host.card.file(), old);
        // A sealed line from an X4 card is absent and kept by a remove.
        let mut file = v.host.card.file();
        file.extend_from_slice(b"mail.sealed\t\t\ts1:00aa\n");
        v.host.card.put(file::FILE, &file);
        assert_eq!(v.get("mail", "sealed").err(), Some(ERR_NOT_FOUND));
    });
}

#[test]
fn a_failed_upgrade_leaves_the_veiled_lines_as_they_are() {
    let old = interface6(&[("mail.password", "mail.password", false, "old-pw")]);
    with_host(host(Seal::new(9)), true, |v| {
        v.host.card.put(file::FILE, &old);
        v.host.card.fail_rename.set(true);
        assert!(v.services().upgrade().is_err());
        assert_eq!(v.host.card.file(), old);
        v.host.card.fail_rename.set(false);
        assert_eq!(v.services().upgrade(), Ok(true));
        assert!(text(&v.host.card.file()).contains("s1:"));
    });
}

#[test]
fn an_upgrade_with_weak_numbers_changes_nothing_and_says_so_until_they_are_strong() {
    let old = interface6(&[("mail.password", "mail.password", false, "old-pw")]);
    let seal = Seal::new(9);
    let weak = seal.weak.clone();
    with_host(host(seal), true, |v| {
        v.host.card.put(file::FILE, &old);
        // A power cut left a part of a rewrite: it is no file of ours.
        v.host.card.put(file::TEMP, b"mail.password\t\t\ts1:00");
        weak.set(true);
        // At start, the radio off: nothing sealed, the shell tries again.
        assert_eq!(v.services().upgrade(), Err(ERR_STATE));
        assert_eq!(v.host.card.file(), old);
        assert_eq!(v.get("mail", "password").unwrap().0, "old-pw");
        // The radio on: sealed, and the value reads as before.
        weak.set(false);
        assert_eq!(v.services().upgrade(), Ok(true));
        assert!(text(&v.host.card.file()).contains("s1:"));
        assert_eq!(v.get("mail", "password").unwrap().0, "old-pw");
        assert_eq!(v.services().upgrade(), Ok(false), "once");
    });
}

#[test]
fn a_sealed_line_is_not_read_by_interface_6_and_stays_for_it() {
    with_view(|v| {
        v.keep("mail", "password", "", "pw").unwrap();
        let secrets = paperpaw_sdk::secrets::Secrets::decode(&text(&v.host.card.file()));
        assert_eq!(secrets.get("mail.password"), None);
        assert!(
            secrets.encode().contains("s1:"),
            "kept unread, written back"
        );
    });
}

#[test]
fn only_the_owner_reads_a_value_the_editor_lists_and_changes_what_the_feature_allows() {
    with_view(|v| {
        v.keep_flags("mail", "", "password", "mail.password", "pw", 1)
            .unwrap();
        v.keep("kosync", "key", "", "k").unwrap();
        // get: another plugin's value is denied, also for the editor.
        let ask = Record::ServiceSecName(wire::ServiceSecName {
            plugin: "mail",
            name: "password",
        });
        assert_eq!(v.call("settings", editor(), 1, ask).err(), Some(ERR_DENIED));
        // list all needs the feature; unwired is unsupported, unadmitted denied.
        assert_eq!(
            v.list("settings", NONE, true, 0).err(),
            Some(ERR_UNSUPPORTED)
        );
        assert_eq!(
            v.list("settings", AdmittedFeatures::test(0, WIRED), true, 0)
                .err(),
            Some(ERR_DENIED)
        );
        let (rows, _, last) = v.list("settings", editor(), true, 0).unwrap();
        assert!(last);
        assert_eq!(
            rows,
            [
                ("mail".into(), "password".into(), "mail.password".into(), 1),
                ("kosync".into(), "key".into(), "".into(), 0)
            ]
        );
        // The own list holds only the caller's.
        assert_eq!(v.list("mail", NONE, false, 0).unwrap().0.len(), 1);
        // The editor replaces a changeable one and cannot change the flag.
        v.keep_flags("settings", "mail", "password", "", "new", 0)
            .unwrap();
        assert_eq!(
            v.get("mail", "password").unwrap(),
            ("new".into(), "mail.password".into(), 1)
        );
        // Not a changeable one, not a missing one, not without the feature.
        assert_eq!(
            v.keep_flags("settings", "kosync", "key", "", "x", 0).err(),
            Some(ERR_DENIED)
        );
        assert_eq!(
            v.keep_flags("settings", "kosync", "nope", "", "x", 0).err(),
            Some(ERR_NOT_FOUND)
        );
        let keep = Record::ServiceSecKeep(wire::ServiceSecKeep {
            plugin: "mail",
            name: "password",
            label: "",
            flags: 0,
            value: "x",
        });
        assert_eq!(
            v.call("settings", NONE, 3, keep).err(),
            Some(ERR_UNSUPPORTED)
        );
        assert_eq!(
            v.call(
                "settings",
                AdmittedFeatures::test(0, WIRED),
                3,
                Record::ServiceSecKeep(wire::ServiceSecKeep {
                    plugin: "mail",
                    name: "password",
                    label: "",
                    flags: 0,
                    value: "x"
                })
            )
            .err(),
            Some(ERR_DENIED)
        );
        // The editor removes any, changeable or not; removing nothing is OK.
        v.remove("settings", "kosync", "key").unwrap();
        v.remove("settings", "kosync", "key").unwrap();
        assert_eq!(v.get("kosync", "key").err(), Some(ERR_NOT_FOUND));
    });
}

#[test]
fn a_plugin_names_itself_or_nothing_and_a_removal_by_plugin_id_drops_every_line_of_it() {
    with_view(|v| {
        v.keep("mail", "a", "", "1").unwrap();
        v.keep("mail", "b", "", "2").unwrap();
        v.keep("wifi", "a", "", "3").unwrap();
        // Naming oneself is the same as "" (no feature needed).
        v.call(
            "mail",
            NONE,
            3,
            Record::ServiceSecKeep(wire::ServiceSecKeep {
                plugin: "mail",
                name: "c",
                label: "",
                flags: 0,
                value: "own",
            }),
        )
        .unwrap();
        assert_eq!(v.get("mail", "c").unwrap().0, "own");
        // Uninstall with the data: by the validated id, also lines that do not open.
        let mut file = v.host.card.file();
        file.extend_from_slice(b"mail.foreign\t\t\ts1:00\n");
        v.host.card.put(file::FILE, &file);
        v.services().forget_plugin("mail").unwrap();
        assert_eq!(v.names("mail"), Vec::<String>::new());
        assert!(!text(&v.host.card.file()).contains("mail."));
        assert_eq!(v.get("wifi", "a").unwrap().0, "3");
        assert_eq!(v.services().forget_plugin("a.b").err(), Some(ERR_INVALID));
    });
}

#[test]
fn list_pages_in_file_order_with_a_cursor_and_a_last_flag() {
    with_view(|v| {
        for owner in ["wifi", "mail"] {
            for n in (0..40).rev() {
                let label = "l".repeat(60);
                v.keep(owner, &std::format!("n{n:02}"), &label, "v").ok();
            }
        }
        // 32 per plugin: the first 32 of 40 (n39 down to n08) fit; wifi's lines
        // were appended first, then mail's, and a page follows the file.
        let (page, next, last) = v.list("settings", editor(), true, 0).unwrap();
        assert_eq!(page.len(), 32);
        assert!(!last);
        assert_eq!(next, 32);
        assert!(page.iter().all(|r| r.0 == "wifi"));
        assert_eq!(page[0].1, "n39");
        assert_eq!(page[31].1, "n08");
        assert_eq!(v.total.get(), 64, "the rows the caller may see");
        let (rest, next, last) = v.list("settings", editor(), true, next).unwrap();
        assert!(last);
        assert_eq!(rest.len(), 32);
        assert!(rest.iter().all(|r| r.0 == "mail"));
        assert_eq!(next, 64);
        // A changed secret keeps its line; the cursor counts rows the caller sees.
        v.keep("wifi", "n39", "", "again").unwrap();
        assert_eq!(v.list("settings", editor(), true, 0).unwrap().0[0].1, "n39");
        assert_eq!(v.list("mail", NONE, false, 30).unwrap().0.len(), 2);
        // Past the end: no rows, last.
        let (none, next, last) = v.list("settings", editor(), true, 99).unwrap();
        assert!((none.is_empty() && last) && next == 99);
        assert_eq!(v.names("mail").len(), 32);
    });
}

#[test]
fn a_label_follows_the_name_rule() {
    with_view(|v| {
        assert_eq!(
            v.keep("mail", "a", "bad\u{7}label", "v").err(),
            Some(ERR_INVALID)
        );
        assert_eq!(
            v.keep("mail", "a", "c1\u{85}", "v").err(),
            Some(ERR_INVALID)
        );
        assert_eq!(v.keep("mail", "a\u{7f}", "", "v").err(), Some(ERR_INVALID));
        v.keep("mail", "a", "tab\there", "v").unwrap();
    });
}

#[test]
fn a_page_holds_whole_rows_that_fit_the_reply() {
    with_view(|v| {
        let name = "n".repeat(64);
        let label = "l".repeat(64);
        for owner in 0..8 {
            for n in 0..4 {
                let owner = std::format!("plugin-{}-{}", "o".repeat(40), owner);
                v.keep(&owner, &std::format!("{n}{}", &name[1..]), &label, "v")
                    .unwrap();
            }
        }
        let mut cursor = 0;
        let mut total = 0;
        loop {
            let (rows, next, last) = v.list("settings", editor(), true, cursor).unwrap();
            assert!(!rows.is_empty());
            total += rows.len();
            if last {
                break;
            }
            cursor = next;
        }
        assert_eq!(total, 32);
    });
}

#[test]
fn random_draws_one_to_256_bytes_with_the_strong_flag_and_never_a_weaker_stand_in() {
    with_view(|v| {
        let draw = |len: u32| {
            v.call(
                "lock",
                NONE,
                5,
                Record::ServiceSecRandom(wire::ServiceSecRandom { len }),
            )
        };
        for bad in [0, 257, u32::MAX] {
            assert_eq!(draw(bad).err(), Some(ERR_INVALID), "{bad}");
        }
        for len in [1, 16, 256] {
            let reply = draw(len).unwrap();
            let Record::ServiceSecBytes(b) = wire::record_decode_one(&reply).unwrap() else {
                panic!()
            };
            assert_eq!((b.data.len(), b.flags), (len as usize, 1));
        }
        v.host.source.strong.set(false);
        let reply = draw(4).unwrap();
        let Record::ServiceSecBytes(b) = wire::record_decode_one(&reply).unwrap() else {
            panic!()
        };
        assert_eq!(b.flags, 0);
        // Random works for a plugin whose id the file cannot hold, and needs no file.
        assert!(
            v.call(
                "core.x",
                NONE,
                5,
                Record::ServiceSecRandom(wire::ServiceSecRandom { len: 1 })
            )
            .is_ok()
        );
        assert!(v.host.card.get(file::FILE).is_none());
    });
    let mut none = host(Seal::new(1));
    Rc::get_mut(&mut none).unwrap().has_source = false;
    with_host(none, true, |v| {
        assert_eq!(
            v.call(
                "lock",
                NONE,
                5,
                Record::ServiceSecRandom(wire::ServiceSecRandom { len: 4 })
            )
            .err(),
            Some(ERR_UNSUPPORTED)
        );
    });
}

#[test]
fn malformed_input_is_refused_and_leaves_the_card_alone() {
    with_view(|v| {
        v.keep("mail", "a", "", "1").unwrap();
        let before = v.host.card.file();
        let schema = services_wire::service_schema("core.secrets", 1).unwrap();
        for op in 1..=5 {
            let op = schema.op(op).unwrap();
            for input in [&[][..], &[0xff; 8], &[1, 2, 3], &[0; 300][..]] {
                assert!(v.call_raw("mail", NONE, op, input).is_err(), "op {}", op.id);
            }
        }
        // A record of another op's tag.
        assert_eq!(
            v.call(
                "mail",
                NONE,
                1,
                Record::ServiceSecRandom(wire::ServiceSecRandom { len: 1 })
            )
            .err(),
            Some(ERR_INVALID)
        );
        assert_eq!(v.host.card.file(), before);
        assert!(v.host.card.get(file::TEMP).is_none());
    });
}

#[test]
fn a_failed_write_or_rename_leaves_the_old_file_and_no_new_one() {
    with_view(|v| {
        v.keep("mail", "a", "", "1").unwrap();
        let before = v.host.card.file();
        v.host.card.fail_rename.set(true);
        assert_eq!(v.keep("mail", "b", "", "2").err(), Some(ERR_IO));
        assert_eq!(v.remove("mail", "", "a").err(), Some(ERR_IO));
        v.host.card.fail_rename.set(false);
        for left in 0..2 {
            v.host.card.appends_left.set(left);
            assert!(v.keep("mail", "b", "", "2").is_err(), "{left}");
            assert_eq!(v.host.card.file(), before);
            assert!(v.host.card.get(file::TEMP).is_none());
        }
        v.host.card.appends_left.set(u32::MAX);
        assert_eq!(v.get("mail", "b").err(), Some(ERR_NOT_FOUND));
        v.keep("mail", "b", "", "2").unwrap();
        assert_eq!(v.get("mail", "b").unwrap().0, "2");
    });
}

#[test]
fn an_unreadable_file_is_never_overwritten_and_the_notice_comes_once() {
    with_view(|v| {
        v.keep("mail", "a", "", "1").unwrap();
        let before = v.host.card.file();
        v.host.card.fail_read.set(true);
        assert_eq!(v.keep("mail", "b", "", "2").err(), Some(ERR_IO));
        assert_eq!(v.remove("mail", "", "a").err(), Some(ERR_IO));
        assert_eq!(v.get("mail", "a").err(), Some(ERR_IO));
        assert_eq!(v.host.notices.get(), 1, "told once");
        v.host.card.fail_read.set(false);
        assert_eq!(v.host.card.file(), before);
        assert_eq!(v.get("mail", "a").unwrap().0, "1");
    });
}

#[test]
fn a_missing_file_is_an_empty_one_and_remove_of_nothing_writes_nothing() {
    with_view(|v| {
        assert_eq!(v.get("mail", "a").err(), Some(ERR_NOT_FOUND));
        assert_eq!(v.list("mail", NONE, false, 0).unwrap().0.len(), 0);
        v.remove("mail", "", "a").unwrap();
        assert!(v.host.card.get(file::FILE).is_none());
        assert!(v.host.card.get(file::TEMP).is_none());
        assert_eq!(v.services().upgrade(), Ok(false));
    });
}

#[test]
fn a_large_file_streams_through_a_small_heap() {
    with_view(|v| {
        // About 59 KiB: 9 plugins of 32 names with 60-byte values.
        let mut file = Vec::new();
        for owner in 0..9 {
            for n in 0..32 {
                let mut buf = Buf::new(LINE_MAX + 1).unwrap();
                file::build(
                    &mut buf,
                    &mut Buf::new(file::SEALED_MAX).unwrap(),
                    &v.host.seal,
                    (
                        &std::format!("p{owner:02}"),
                        &std::format!("n{n:02}"),
                        "label",
                        false,
                    ),
                    &[b'v'; 60],
                )
                .unwrap();
                file.extend_from_slice(buf.as_bytes());
            }
        }
        assert!(
            file.len() > 50_000 && (file.len() as u64) < file::FILE_MAX,
            "{}",
            file.len()
        );
        v.host.card.put(file::FILE, &file);
        let held = |f: &dyn Fn()| {
            f();
            v.held.get()
        };
        let get = held(&|| assert_eq!(v.get("p08", "n31").unwrap().0, "v".repeat(60)));
        let list = held(&|| {
            assert_eq!(v.list("p05", NONE, false, 0).unwrap().0.len(), 32);
        });
        let keep = held(&|| v.keep("p04x", "extra", "", "new").unwrap());
        let remove = held(&|| v.remove("p07", "", "n03").unwrap());
        for (what, bytes) in [
            ("get", get),
            ("list", list),
            ("keep", keep),
            ("remove", remove),
        ] {
            // The read buffer, two value buffers, and a line for a rewrite: about
            // 7.5 KiB for a 59 KiB file.
            assert!(bytes < 8 * 1024, "{what} held {bytes} bytes");
        }
        // `get` and `list` (the settings screen's calls) hold no line buffer.
        for (what, bytes) in [("get", get), ("list", list)] {
            assert!(bytes < 5 * 1024 + 512, "{what} held {bytes} bytes");
        }
        assert!(v.host.card.largest_read.get() <= READ_BUF);
        assert_eq!(v.get("p04x", "extra").unwrap().0, "new");
        assert_eq!(v.get("p07", "n03").err(), Some(ERR_NOT_FOUND));
    });
}

#[test]
fn every_allocation_that_fails_refuses_cleanly_and_changes_nothing() {
    with_view(|v| {
        v.keep("mail", "a", "", "1").unwrap();
        let before = v.host.card.file();
        let mut nth = 1;
        loop {
            v.fail.set(nth);
            let result = v.keep("mail", "b", "", "2");
            v.fail.set(0);
            match result {
                Ok(()) => break,
                Err(e) => {
                    assert_eq!(e, ERR_NO_MEMORY, "allocation {nth}");
                    assert_eq!(v.host.card.file(), before, "allocation {nth}");
                    assert!(v.host.card.get(file::TEMP).is_none());
                }
            }
            nth += 1;
            assert!(nth < 64);
        }
        assert!(nth > 3, "the call allocates its buffers ({nth})");
        // The reads and the other writes refuse the same way.
        for n in 1..=nth {
            v.fail.set(n);
            let results = [
                v.get("mail", "b").err(),
                v.list("mail", NONE, false, 0).err(),
                v.remove("mail", "", "b").err(),
            ];
            v.fail.set(0);
            for e in results.into_iter().flatten() {
                assert_eq!(e, ERR_NO_MEMORY, "allocation {n}");
            }
        }
        v.keep("mail", "b", "", "2").unwrap();
        assert_eq!(v.get("mail", "b").unwrap().0, "2");
    });
}

#[test]
fn values_never_show_in_a_debug_print() {
    let keep = Record::ServiceSecKeep(wire::ServiceSecKeep {
        plugin: "",
        name: "password",
        label: "",
        flags: 0,
        value: "hunter2-visible?",
    });
    assert!(!std::format!("{keep:?}").contains("hunter2"));
    let value = Record::ServiceSecValue(wire::ServiceSecValue {
        flags: 0,
        label: "",
        value: "hunter2-visible?",
    });
    assert!(!std::format!("{value:?}").contains("hunter2"));
    let mut buf = Buf::new(32).unwrap();
    buf.push(b"hunter2").unwrap();
    assert!(!std::format!("{buf:?}").contains("hunter2"));
    drop(buf);
}

#[test]
fn the_file_name_is_the_sdks() {
    assert_eq!(file::FILE, paperpaw_sdk::secrets::FILE);
}

#[test]
fn an_unknown_list_flag_bit_is_unsupported() {
    with_view(|v| {
        for flags in [2, 0x8000_0000, 3] {
            assert_eq!(
                v.call(
                    "mail",
                    NONE,
                    2,
                    Record::ServiceSecList(wire::ServiceSecList { flags, cursor: 0 })
                )
                .err(),
                Some(ERR_UNSUPPORTED),
                "{flags:#x}"
            );
        }
    });
}

#[test]
fn get_takes_an_empty_plugin_only_and_an_explicit_id_is_denied_even_the_callers_own() {
    with_view(|v| {
        v.keep("mail", "password", "", "pw").unwrap();
        let named = |plugin: &'static str| {
            Record::ServiceSecName(wire::ServiceSecName {
                plugin,
                name: "password",
            })
        };
        // "" succeeds.
        assert_eq!(v.get("mail", "password").unwrap().0, "pw");
        // The caller's own id, a foreign id, with and without the feature.
        for (caller, features, plugin) in [
            ("mail", NONE, "mail"),
            ("mail", editor(), "mail"),
            ("settings", editor(), "mail"),
            ("other", NONE, "mail"),
        ] {
            assert_eq!(
                v.call(caller, features, 1, named(plugin)).err(),
                Some(ERR_DENIED),
                "{caller} asks for {plugin}"
            );
        }
    });
}

#[test]
fn a_line_of_line_max_is_read_and_one_byte_more_is_handed_over_whole() {
    let mut file = Vec::new();
    file.extend_from_slice(&[b'a'; LINE_MAX]);
    file.push(b'\n');
    file.extend_from_slice(&[b'b'; LINE_MAX + 1]);
    file.push(b'\n');
    file.extend_from_slice(b"ok\n");
    with_view(|v| {
        v.host.card.put(file::FILE, &file);
        let mut buf = Buf::new(READ_BUF).unwrap();
        let mut pieces: Vec<(bool, usize, bool)> = Vec::new();
        file::scan(&v.host.card, file::FILE, &mut buf, &mut |piece| {
            pieces.push(match piece {
                Piece::Line(line) => (true, line.len(), true),
                Piece::Opaque { bytes, end } => (false, bytes.len(), end),
            });
            Ok(Flow::Go)
        })
        .unwrap();
        // The first line whole, with its break; the longer one in opaque
        // parts that add up to it, the last marked; the short line after it.
        assert_eq!(pieces[0], (true, LINE_MAX + 1, true));
        let (parts, last) = (&pieces[1..pieces.len() - 1], pieces[pieces.len() - 1]);
        assert!(parts.iter().all(|(line, _, _)| !line));
        assert_eq!(
            parts.iter().map(|(_, len, _)| len).sum::<usize>(),
            LINE_MAX + 1
        );
        assert!(parts[parts.len() - 1].2);
        assert!(parts[..parts.len() - 1].iter().all(|(_, _, end)| !end));
        assert_eq!(last, (true, 3, true));
    });
}

#[test]
fn a_secret_after_an_over_long_line_is_still_read_and_the_long_line_kept() {
    let long = std::format!("mail.long\t\t\t{}\n", "ab".repeat(LINE_MAX));
    let mut file = long.clone().into_bytes();
    file.extend_from_slice(&interface6(&[("mail.password", "L", true, "pw")]));
    with_view(|v| {
        v.host.card.put(file::FILE, &file);
        assert_eq!(v.get("mail", "password").unwrap().0, "pw");
        assert_eq!(v.names("mail"), ["password"]);
        v.keep("mail", "other", "", "o").unwrap();
        let after = v.host.card.file();
        assert!(
            after.starts_with(long.as_bytes()),
            "the long line is kept first"
        );
        assert_eq!(v.get("mail", "password").unwrap().0, "pw");
    });
}
