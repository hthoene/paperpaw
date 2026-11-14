#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Service-level tests of `core.files` over a fake card that never allocates
//! while it answers (fixed tables), so an allocation failure armed around a
//! call can only be the service's own. The shared harness is here; the ops'
//! tests are in `tests/`.
extern crate std;
use super::*;
use crate::abi7::services_wire::{self, ServiceOp};
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    call_services::validate_reply,
    lib_service::moves::{Access, Entry, MoveHost, Moves, Phase},
};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_platform::{DirEntry, Instant, StorageError};
use paperpaw_sdk::{Env, Settings};

#[path = "tests/book.rs"]
mod book;
#[path = "tests/busy.rs"]
mod busy;
#[path = "tests/change.rs"]
mod change;
#[path = "tests/dispatch.rs"]
mod dispatch;
#[path = "tests/hostile.rs"]
mod hostile;
#[path = "tests/oom.rs"]
mod oom;
#[path = "tests/peer_code.rs"]
mod peer_code;
#[path = "tests/peer_data.rs"]
mod peer_data;
#[path = "tests/read.rs"]
mod read;

pub(super) const OWNER: u64 = 7;
pub(super) const PLUGIN: &str = "files";
/// The folder of the plugin's data on the card.
pub(super) const DATA: &str = ".paperpaw/plugins/files";
/// The folder of the plugin's install files.
pub(super) const BUNDLE: &str = "plugins/files";

const NODES: usize = 320;
const PATH: usize = 900;

#[derive(Clone, Copy)]
struct Node {
    used: bool,
    path: [u8; PATH],
    len: usize,
    folder: bool,
    size: u64,
    serial: u64,
}

impl Node {
    const FREE: Self = Self {
        used: false,
        path: [0; PATH],
        len: 0,
        folder: false,
        size: 0,
        serial: 0,
    };
    fn path(&self) -> &str {
        core::str::from_utf8(&self.path[..self.len]).unwrap()
    }
    fn set(&mut self, path: &str) {
        self.path[..path.len()].copy_from_slice(path.as_bytes());
        self.len = path.len();
    }
}

/// A card of folders and files in tables of fixed size. Places are serial
/// numbers, as FAT's slots; names are compared in any case when `fold`.
pub(super) struct Card {
    nodes: RefCell<std::boxed::Box<[Node; NODES]>>,
    serial: Cell<u64>,
    pub(super) fold: Cell<bool>,
    pub(super) gone: Cell<bool>,
    /// The next create or rename answers `Full`.
    pub(super) full: Cell<bool>,
    pub(super) free: Cell<Option<u64>>,
    pub(super) total: Cell<Option<u64>>,
    /// Counts of the storage calls that change the card.
    pub(super) changes: Cell<u32>,
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(parent, _)| parent)
}

impl Card {
    pub(super) fn new() -> Self {
        Self {
            nodes: RefCell::new(std::boxed::Box::new([Node::FREE; NODES])),
            serial: Cell::new(1),
            fold: Cell::new(true),
            gone: Cell::new(false),
            full: Cell::new(false),
            free: Cell::new(Some(100 << 20)),
            total: Cell::new(Some(1 << 30)),
            changes: Cell::new(0),
        }
    }

    fn same(&self, a: &str, b: &str) -> bool {
        if self.fold.get() {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    }

    fn find(&self, path: &str) -> Option<usize> {
        self.nodes
            .borrow()
            .iter()
            .position(|n| n.used && self.same(n.path(), path))
    }

    fn insert(&self, path: &str, folder: bool, size: u64) -> usize {
        let mut nodes = self.nodes.borrow_mut();
        let at = nodes.iter().position(|n| !n.used).expect("card table full");
        let serial = self.serial.get();
        self.serial.set(serial + 1);
        nodes[at] = Node::FREE;
        nodes[at].used = true;
        nodes[at].set(path);
        nodes[at].folder = folder;
        nodes[at].size = size;
        nodes[at].serial = serial;
        at
    }

    /// Makes `path` and its folders, as the host's own set-up (no checks).
    pub(super) fn dir(&self, path: &str) {
        let mut at = 0;
        loop {
            let end = path[at..].find('/').map_or(path.len(), |i| at + i);
            let part = &path[..end];
            if !part.is_empty() && self.find(part).is_none() {
                self.insert(part, true, 0);
            }
            if end == path.len() {
                break;
            }
            at = end + 1;
        }
    }

    pub(super) fn file(&self, path: &str, size: u64) {
        self.dir(parent(path));
        match self.find(path) {
            Some(at) => self.nodes.borrow_mut()[at].size = size,
            None => {
                self.insert(path, false, size);
            }
        }
    }

    pub(super) fn has(&self, path: &str) -> bool {
        self.find(path).is_some()
    }

    pub(super) fn is_dir(&self, path: &str) -> bool {
        self.find(path)
            .is_some_and(|at| self.nodes.borrow()[at].folder)
    }

    pub(super) fn count(&self) -> usize {
        self.nodes.borrow().iter().filter(|n| n.used).count()
    }

    pub(super) fn paths(&self) -> Vec<std::string::String> {
        let mut paths: Vec<_> = self
            .nodes
            .borrow()
            .iter()
            .filter(|n| n.used)
            .map(|n| std::string::String::from(n.path()))
            .collect();
        paths.sort();
        paths
    }

    fn check(&self) -> Result<(), StorageError> {
        if self.gone.get() {
            Err(StorageError::CARD_GONE)
        } else {
            Ok(())
        }
    }

    /// The folder `dir`: `Ok` if it is one (the root always), `InvalidPath`
    /// for a file, `NotFound` for none.
    fn folder(&self, dir: &str) -> Result<(), StorageError> {
        if dir.is_empty() {
            return Ok(());
        }
        match self.find(dir) {
            Some(at) if self.nodes.borrow()[at].folder => Ok(()),
            Some(_) => Err(StorageError::InvalidPath),
            None => {
                // A file above makes it `InvalidPath`, as FAT.
                let mut up = parent(dir);
                while !up.is_empty() {
                    if self
                        .find(up)
                        .is_some_and(|at| !self.nodes.borrow()[at].folder)
                    {
                        return Err(StorageError::InvalidPath);
                    }
                    up = parent(up);
                }
                Err(StorageError::NotFound)
            }
        }
    }

    fn make_dirs(&self, path: &str) -> Result<(), StorageError> {
        let mut at = 0;
        loop {
            let end = path[at..].find('/').map_or(path.len(), |i| at + i);
            let part = &path[..end];
            match self.find(part) {
                Some(n) if self.nodes.borrow()[n].folder => {}
                Some(_) => return Err(StorageError::InvalidPath),
                None => {
                    if self.full.get() {
                        return Err(StorageError::Full);
                    }
                    self.insert(part, true, 0);
                }
            }
            if end == path.len() {
                return Ok(());
            }
            at = end + 1;
        }
    }

    fn within(&self, path: &str, folder: &str) -> bool {
        super::target::within(self.fold.get(), path, folder)
    }
}

impl Storage for Card {
    fn read_at(&self, path: &str, _: u64, _: &mut [u8]) -> Result<usize, StorageError> {
        self.size(path).map(|_| 0)
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.check()?;
        self.folder(parent(path))?;
        match self.find(path) {
            Some(at) if !self.nodes.borrow()[at].folder => Ok(self.nodes.borrow()[at].size),
            Some(_) => Err(StorageError::InvalidPath),
            None => Err(StorageError::NotFound),
        }
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.check()?;
        self.make_dirs(parent(path))?;
        self.changes.set(self.changes.get() + 1);
        self.file(path, data.len() as u64);
        Ok(())
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.write(path, data)
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.list_from(dir, 0, &mut |entry| {
            visit(entry);
            true
        })
        .map(|_| ())
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        self.check()?;
        self.folder(dir)?;
        let mut at = from;
        loop {
            // The next entry of the folder at or after place `at`.
            let found = {
                let nodes = self.nodes.borrow();
                nodes
                    .iter()
                    .filter(|n| n.used && n.serial >= at && self.same(parent(n.path()), dir))
                    .min_by_key(|n| n.serial)
                    .copied()
            };
            let Some(node) = found else {
                return Ok(None);
            };
            let name = node.path().rsplit_once('/').map_or(node.path(), |(_, n)| n);
            let entry = DirEntry::new(name, node.folder, node.size)
                .with_modified(Some(1_000 + node.serial as i64));
            if !visit(entry) {
                return Ok(Some(node.serial));
            }
            at = node.serial + 1;
        }
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.check()?;
        self.changes.set(self.changes.get() + 1);
        let Some(at) = self.find(path) else {
            return Ok(());
        };
        if self.nodes.borrow()[at].folder {
            let nodes = self.nodes.borrow();
            let child = nodes
                .iter()
                .any(|n| n.used && n.len > path.len() && self.within(n.path(), path));
            drop(nodes);
            if child {
                return Err(StorageError::InvalidPath);
            }
        }
        self.nodes.borrow_mut()[at] = Node::FREE;
        Ok(())
    }

    fn remove_listed(&self, dir: &str, place: u64) -> Result<(), StorageError> {
        self.check()?;
        self.folder(dir)?;
        let node = {
            let nodes = self.nodes.borrow();
            nodes
                .iter()
                .filter(|n| n.used && n.serial >= place && self.same(parent(n.path()), dir))
                .min_by_key(|n| n.serial)
                .copied()
        };
        let node = node.ok_or(StorageError::NotFound)?;
        if node.folder
            && self
                .nodes
                .borrow()
                .iter()
                .any(|n| n.used && n.len > node.len && self.within(n.path(), node.path()))
        {
            return Err(StorageError::InvalidPath);
        }
        self.changes.set(self.changes.get() + 1);
        let at = self.find(node.path()).unwrap();
        self.nodes.borrow_mut()[at] = Node::FREE;
        Ok(())
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        self.check()?;
        self.changes.set(self.changes.get() + 1);
        let mut nodes = self.nodes.borrow_mut();
        for n in nodes.iter_mut() {
            if n.used && self.within(n.path(), path) {
                *n = Node::FREE;
            }
        }
        Ok(())
    }

    fn modified(&self, path: &str) -> Option<i64> {
        let at = self.find(path)?;
        Some(1_000 + self.nodes.borrow()[at].serial as i64)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        self.check()?;
        let src = self.find(from).ok_or(StorageError::NotFound)?;
        let src_folder = self.nodes.borrow()[src].folder;
        let case_only = self.fold.get() && self.same(from, to);
        if !case_only && let Some(at) = self.find(to) {
            if self.nodes.borrow()[at].folder || src_folder {
                return Err(StorageError::InvalidPath);
            }
            self.nodes.borrow_mut()[at] = Node::FREE;
        }
        if !case_only {
            self.make_dirs(parent(to))?;
        }
        if self.full.get() {
            return Err(StorageError::Full);
        }
        self.changes.set(self.changes.get() + 1);
        let mut nodes = self.nodes.borrow_mut();
        for i in 0..NODES {
            if !nodes[i].used || !super::target::within(self.fold.get(), nodes[i].path(), from) {
                continue;
            }
            // The new path in a stack buffer: the card never allocates.
            let mut buf = [0u8; PATH];
            let rest = nodes[i].len - from.len();
            buf[..to.len()].copy_from_slice(to.as_bytes());
            buf[to.len()..to.len() + rest]
                .copy_from_slice(&nodes[i].path[from.len()..nodes[i].len]);
            let new = core::str::from_utf8(&buf[..to.len() + rest]).unwrap();
            nodes[i].set(new);
        }
        Ok(())
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        self.check()?;
        self.changes.set(self.changes.get() + 1);
        self.make_dirs(path)
    }

    fn ignores_case(&self) -> bool {
        self.fold.get()
    }

    fn free_space(&self) -> Option<u64> {
        self.free.get()
    }

    fn total_space(&self) -> Option<u64> {
        self.total.get()
    }
}

/// Features the fixture's host wires.
const WIRED: &[paperpaw_native::metadata::Feature<'static>] =
    &[paperpaw_native::metadata::Feature {
        key: "uses.storage",
        revision: 1,
    }];

/// The host: the card's state the service does not know, the paths in use,
/// and the library's move job (the real [`Moves`] over the fake card).
pub(super) struct TestHost {
    pub(super) card: Rc<Card>,
    pub(super) moves: Moves,
    /// Paths in use: reading is fine, changes are busy.
    pub(super) in_use: RefCell<Vec<std::string::String>>,
    /// Aliases the host resolves: (root, alias, name).
    pub(super) aliases: RefCell<Vec<(u32, &'static str)>>,
    pub(super) journal: RefCell<Option<(std::string::String, std::string::String, bool, u64)>>,
    pub(super) library_moves: Cell<u32>,
    /// Plugins built into the firmware.
    pub(super) built_in: RefCell<Vec<&'static str>>,
    /// Folders (below `plugins/`) of the running data plugins.
    pub(super) data_folders: RefCell<Vec<&'static str>>,
}

impl TestHost {
    pub(super) fn new(card: &Rc<Card>) -> Rc<Self> {
        Rc::new(Self {
            card: Rc::clone(card),
            moves: Moves::default(),
            in_use: RefCell::default(),
            aliases: RefCell::default(),
            journal: RefCell::default(),
            library_moves: Cell::new(0),
            built_in: RefCell::default(),
            data_folders: RefCell::default(),
        })
    }

    /// One bounded step of the library's job.
    pub(super) fn step(&self) -> bool {
        self.moves.step(&Library(self)).unwrap()
    }

    pub(super) fn finish_job(&self) {
        while self.step() {}
    }
}

impl FilesHost for TestHost {
    fn built_in(&self, plugin: &str) -> bool {
        self.built_in.borrow().contains(&plugin)
    }

    fn data_folder(&self, name: &str) -> bool {
        // As the real set: names compared case folded.
        self.data_folders
            .borrow()
            .iter()
            .any(|have| have.eq_ignore_ascii_case(name))
    }

    fn host_file(&self, folder: &str, entry: &str) -> bool {
        crate::native_host::data_plugin_folders::host_file(&*self.card, folder, entry)
    }

    fn excluded(&self, root: u32, _parent: &str, first: &str) -> bool {
        self.aliases
            .borrow()
            .iter()
            .any(|(r, alias)| *r == root && alias.eq_ignore_ascii_case(first))
    }

    fn busy(&self, path: &str, access: Access) -> bool {
        self.in_use.borrow().iter().any(|used| {
            // Held open: the path, a folder holding it (any change), or a
            // path below a folder (removed with it).
            self.card.within(used, path)
                || (access == Access::Remove && self.card.within(path, used))
        }) || self.moves.busy(&Library(self), path, access)
    }

    fn moving(&self) -> bool {
        self.moves.active()
    }

    fn rename(
        &self,
        storage: &dyn Storage,
        from: &str,
        to: &str,
        replace: bool,
        library: bool,
    ) -> Result<(), i32> {
        if library {
            self.library_moves.set(self.library_moves.get() + 1);
            return self.moves.begin(&Library(self), from, to, replace);
        }
        storage
            .rename(from, to)
            .map_err(crate::native_host::storage::status)
    }
}

/// The library's move machinery over the fake card.
struct Library<'a>(&'a TestHost);

impl MoveHost for Library<'_> {
    fn open_entry(&self, visit: &mut dyn FnMut(&Entry<'_>)) -> Result<bool, i32> {
        Ok(self
            .0
            .journal
            .borrow()
            .as_ref()
            .is_some_and(|(f, t, r, i)| {
                visit(&Entry {
                    from: f,
                    to: t,
                    replace: *r,
                    identity: *i,
                    renamed: false,
                });
                true
            }))
    }
    fn write_entry(&self, entry: &Entry<'_>) -> Result<(), i32> {
        *self.0.journal.borrow_mut() = Some((
            entry.from.into(),
            entry.to.into(),
            entry.replace,
            entry.identity,
        ));
        Ok(())
    }
    fn clear_entry(&self) -> Result<(), i32> {
        *self.0.journal.borrow_mut() = None;
        Ok(())
    }
    fn identity(&self, path: &str) -> Result<Option<u64>, i32> {
        Ok(self
            .0
            .card
            .find(path)
            .map(|at| self.0.card.nodes.borrow()[at].serial))
    }
    fn is_folder(&self, path: &str) -> Result<bool, i32> {
        Ok(self.0.card.is_dir(path))
    }
    fn same_entry(&self, a: &str, b: &str) -> Result<bool, i32> {
        Ok(self.0.card.same(a, b) && self.0.card.has(a))
    }
    fn spelled_as(&self, path: &str) -> Result<bool, i32> {
        Ok(self.0.card.paths().iter().any(|p| p == path))
    }
    fn rename(&self, from: &str, to: &str, _replace: bool) -> Result<(), i32> {
        self.0
            .card
            .rename(from, to)
            .map_err(crate::native_host::storage::status)
    }
    fn settle(&self) -> Result<(), i32> {
        Ok(())
    }
    fn within(&self, path: &str, folder: &str) -> bool {
        self.0.card.within(path, folder)
    }
    fn discard_replaced(&self, _to: &str) -> Result<(), i32> {
        Ok(())
    }
    fn carry(&self, _from: &str, _to: &str, phase: Phase) -> Result<Phase, i32> {
        Ok(match phase {
            Phase::Books(_) => Phase::Assignments,
            Phase::Assignments => Phase::Settings,
            Phase::Settings => Phase::Events(0),
            Phase::Events(_) | Phase::Done => Phase::Done,
        })
    }
    fn carry_book(&self, _from: &str, _to: &str, _book: &str) -> Result<(), i32> {
        Ok(())
    }
}

/// A frame whose storage view is the fake card.
pub(super) struct TestFrame<'a> {
    env: &'a Env<'a>,
    card: Option<&'a Card>,
    plugin: &'static str,
    hosts: crate::native_host::host_services::HostServices,
    pub(super) granted: bool,
}

impl Frame for TestFrame<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn storage(&self) -> Option<&dyn Storage> {
        self.card.map(|card| card as &dyn Storage)
    }
}

impl TestFrame<'_> {
    pub(super) fn run(&self, op: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let caller = crate::native_host::host_services::Caller {
            owner: OWNER,
            plugin: self.plugin,
            features: crate::native_host::admitted_features::AdmittedFeatures::test(
                u32::from(self.granted),
                WIRED,
            ),
            hosts: &self.hosts,
        };
        backend(self, &caller, op, input, out)
    }
}

/// How a call is set up.
#[derive(Clone, Copy)]
pub(super) struct Setup {
    pub(super) plugin: &'static str,
    pub(super) granted: bool,
    pub(super) wired: bool,
    pub(super) storage: bool,
    /// The caller is in a format call for this book: kind, path, opened file.
    pub(super) book: Option<(
        crate::native_host::format_scope::BookKind,
        &'static str,
        &'static str,
    )>,
}

pub(super) const NORMAL: Setup = Setup {
    plugin: PLUGIN,
    granted: true,
    wired: true,
    storage: true,
    book: None,
};

pub(super) fn with<R>(
    host: &Rc<TestHost>,
    setup: Setup,
    body: impl FnOnce(&TestFrame<'_>) -> R,
) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = crate::native_host::host_services::HostServices::new();
    if setup.wired {
        hosts.wire_files(host.clone()).unwrap();
    }
    let frame = TestFrame {
        env: &env,
        card: setup.storage.then_some(&*host.card),
        plugin: setup.plugin,
        hosts,
        granted: setup.granted,
    };
    let _guard = setup.book.map(|(kind, path, opened)| {
        frame
            .hosts
            .format_scope()
            .enter(
                OWNER,
                &crate::native_host::format_scope::FormatBook {
                    path,
                    kind,
                    opened,
                    document: 1,
                    cache: "",
                    css: "",
                    cache_cap: 0,
                    quota: None,
                },
            )
            .unwrap()
    });
    body(&frame)
}

pub(super) fn schema_op(id: u32) -> &'static ServiceOp {
    services_wire::service_schema("core.files", 1)
        .unwrap()
        .op(id)
        .unwrap()
}

pub(super) fn input(op: u32, record: Record<'_>) -> Vec<u8> {
    let op = schema_op(op);
    let mut buffer = std::vec![0; 4096];
    let used = wire::records_encode_counted(
        &[record],
        usize::from(op.input_rows),
        op.max_input,
        &mut buffer,
    )
    .unwrap();
    buffer.truncate(used);
    buffer
}

pub(super) fn call(host: &Rc<TestHost>, op: u32, record: Record<'_>) -> Result<Vec<u8>, i32> {
    call_with(host, NORMAL, op, record)
}

pub(super) fn call_with(
    host: &Rc<TestHost>,
    setup: Setup,
    id: u32,
    record: Record<'_>,
) -> Result<Vec<u8>, i32> {
    let op = schema_op(id);
    let bytes = input(id, record);
    let mut out = std::vec![0; op.max_reply];
    let length = with(host, setup, |frame| frame.run(op, &bytes, &mut out))?;
    out.truncate(length);
    validate_reply(&out, op)?;
    Ok(out)
}

pub(super) fn path(root: u32, path: &str) -> Record<'_> {
    Record::ServiceFsPath(wire::ServiceFsPath { root, path })
}

pub(super) fn list_record(root: u32, path: &str, cursor: u64) -> Record<'_> {
    Record::ServiceFsList(wire::ServiceFsList {
        root,
        path,
        cursor,
        flags: 0,
    })
}

pub(super) fn move_record<'a>(
    from_root: u32,
    from: &'a str,
    to_root: u32,
    to: &'a str,
    flags: u32,
) -> Record<'a> {
    Record::ServiceFsMove(wire::ServiceFsMove {
        from_root,
        from,
        to_root,
        to,
        flags,
    })
}

/// A listed row: name, flags, size, modified.
pub(super) type Row = (std::string::String, u32, u64, Option<u64>);

/// A reply read back: the rows, then the end, if any.
pub(super) fn rows(bytes: &[u8]) -> (Vec<Row>, Option<wire::ServiceEnd>) {
    let (mut rows, mut end) = (Vec::new(), None);
    crate::native_host::stage_wire::visit_stream(bytes, |_, framed| {
        match wire::record_decode_one(framed).unwrap() {
            Record::ServiceFsEntry(e) => rows.push((e.name.into(), e.flags, e.size, e.modified)),
            Record::ServiceEnd(e) => end = Some(e),
            other => panic!("{other:?}"),
        }
        Ok(())
    })
    .unwrap();
    (rows, end)
}

/// Every name of a folder by paging with the cursors the service gives.
pub(super) fn list_all(
    host: &Rc<TestHost>,
    root: u32,
    folder: &str,
) -> Result<Vec<std::string::String>, i32> {
    let (mut names, mut cursor, mut pages) = (Vec::new(), 0, 0);
    loop {
        let reply = call(host, 1, list_record(root, folder, cursor))?;
        let (rows, end) = rows(&reply);
        names.extend(rows.into_iter().map(|r| r.0));
        let end = end.unwrap();
        pages += 1;
        assert!(pages < 200);
        if end.flags & 1 != 0 {
            assert_eq!(end.cursor, None);
            return Ok(names);
        }
        cursor = end
            .cursor
            .expect("a page that is not the last carries a cursor");
        assert_ne!(cursor, 0);
    }
}

pub(super) fn fixture() -> (Rc<Card>, Rc<TestHost>) {
    let card = Rc::new(Card::new());
    let host = TestHost::new(&card);
    (card, host)
}

#[test]
fn a_frame_without_the_host_refuses_the_family() {
    let (_, host) = fixture();
    let setup = Setup {
        wired: false,
        ..NORMAL
    };
    assert_eq!(
        call_with(&host, setup, 3, path(0, "")),
        Err(ERR_UNSUPPORTED)
    );
}

#[test]
fn a_call_outside_a_callback_has_no_storage_view() {
    let (_, host) = fixture();
    let setup = Setup {
        storage: false,
        ..NORMAL
    };
    assert_eq!(call_with(&host, setup, 3, path(0, "")), Err(ERR_STATE));
}

impl Card {
    /// Takes a file out as the host's set-up (no checks).
    pub(super) fn nodes_remove(&self, path: &str) {
        if let Some(at) = self.find(path) {
            self.nodes.borrow_mut()[at] = Node::FREE;
        }
    }
}
