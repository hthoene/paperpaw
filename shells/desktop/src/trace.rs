//! What the storage is asked frame by frame (`--trace`), below the kernel
//! as the X4's `trace` sees the card: the calls each frame's drawing made,
//! band by band, how long the drawing took, and the calls made since the
//! frame before (opening a screen, handling a press). Drawn in bands, a
//! screen that reads while it draws reads again in every band.
//!
//! The times are the computer's, many times faster than an X4's; the
//! calls are the same on both.

use std::cell::{Cell, RefCell};

use paperpaw_platform::{DirEntry, Storage, StorageError};

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    /// Frames are kept for [`take_frames`] (see [`record`]).
    static RECORDING: Cell<bool> = const { Cell::new(false) };
    static FRAMES: RefCell<Vec<FrameRecord>> = const { RefCell::new(Vec::new()) };
    static COUNTS: RefCell<Counts> = RefCell::new(Counts::default());
    /// The band being drawn while a frame is recorded, from 0.
    static BAND: Cell<Option<usize>> = const { Cell::new(None) };
    /// The calls of the frame being recorded.
    static CALLS: RefCell<Vec<Call>> = const { RefCell::new(Vec::new()) };
}

/// Calls of one frame kept at most (a picture streamed row by row takes
/// a few hundred).
const MAX_CALLS: usize = 10_000;

/// Calls by what was done, and bytes read.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    pub reads: u32,
    pub bytes: u64,
    /// Sizes asked and whether files exist.
    pub sizes: u32,
    pub lists: u32,
    pub writes: u32,
    pub other: u32,
}

/// A frame drawn while recording (see [`record`]).
#[derive(Debug, Clone, PartialEq)]
pub struct FrameRecord {
    /// The screen shown.
    pub screen: String,
    /// How long the drawing took, on this computer.
    pub millis: f64,
    /// The storage calls of each band, in order.
    pub bands: Vec<u32>,
    /// The calls the drawing made, all bands.
    pub drawing: Counts,
    /// The calls made since the frame before (opening a screen, handling
    /// a press).
    pub before: Counts,
    /// The calls the drawing made, one by one (the first 10 000).
    pub calls: Vec<Call>,
}

/// A storage call made while a frame was drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The band it was made in, from 0.
    pub band: usize,
    /// What was done: `read`, `size`, `exists`, `list`, `write`, `append`,
    /// `remove`, `rename`, `create_dir` or `remove_dir_all`.
    pub op: &'static str,
    /// The file or folder (for `rename`, the old name).
    pub path: String,
    /// Bytes read.
    pub bytes: u64,
}

impl Counts {
    /// All calls.
    pub fn calls(&self) -> u32 {
        self.reads + self.sizes + self.lists + self.writes + self.other
    }

    fn since(&self, before: &Counts) -> Counts {
        Counts {
            reads: self.reads - before.reads,
            bytes: self.bytes - before.bytes,
            sizes: self.sizes - before.sizes,
            lists: self.lists - before.lists,
            writes: self.writes - before.writes,
            other: self.other - before.other,
        }
    }
}

impl std::fmt::Display for Counts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} calls ({} reads of {} KB, {} sizes, {} lists, {} writes, {} other)",
            self.calls(),
            self.reads,
            self.bytes.div_ceil(1024),
            self.sizes,
            self.lists,
            self.writes,
            self.other
        )
    }
}

/// Prints a line for every frame from now on.
pub fn start() {
    ON.with(|on| on.set(true));
}

/// Keeps a [`FrameRecord`] of every frame from now on, printed or not,
/// for tools that look at them (see [`take_frames`]).
pub fn record() {
    RECORDING.with(|on| on.set(true));
}

/// Stops recording frames ([`record`]); those recorded are dropped.
pub fn stop_recording() {
    RECORDING.with(|on| on.set(false));
    drop(take_frames());
}

/// The frames recorded since the last call.
pub fn take_frames() -> Vec<FrameRecord> {
    FRAMES.with(|frames| std::mem::take(&mut *frames.borrow_mut()))
}

fn on() -> bool {
    ON.with(Cell::get) || RECORDING.with(Cell::get)
}

fn count(f: impl FnOnce(&mut Counts)) {
    COUNTS.with(|counts| f(&mut counts.borrow_mut()));
}

/// Notes a call while a frame is recorded.
fn note(op: &'static str, path: &str, bytes: u64) {
    if let Some(band) = BAND.with(Cell::get) {
        // The trace's own memory is not the kernel's.
        crate::budget::untracked(|| {
            CALLS.with(|calls| {
                let mut calls = calls.borrow_mut();
                if calls.len() < MAX_CALLS {
                    calls.push(Call {
                        band,
                        op,
                        path: path.to_string(),
                        bytes,
                    });
                }
            });
        });
    }
}

fn now() -> Counts {
    COUNTS.with(|counts| *counts.borrow())
}

/// A frame being drawn, from [`Frame::start`] until [`Frame::done`].
#[derive(Debug)]
pub struct Frame {
    /// When it began ([`crate::host::micros`]).
    started: u64,
    before: Counts,
    band: Counts,
    bands: Vec<u32>,
}

thread_local! {
    /// The counts when the last frame was done.
    static LAST: RefCell<Counts> = RefCell::new(Counts::default());
}

impl Frame {
    /// A frame begins; `None` while not tracing.
    pub fn start() -> Option<Frame> {
        if RECORDING.with(Cell::get) {
            BAND.with(|band| band.set(Some(0)));
            CALLS.with(|calls| calls.borrow_mut().clear());
        }
        on().then(|| Frame {
            started: crate::host::micros(),
            before: now(),
            band: now(),
            bands: Vec::new(),
        })
    }

    /// A band was drawn.
    pub fn band(&mut self) {
        let counts = now();
        let calls = counts.since(&self.band).calls();
        crate::budget::untracked(|| self.bands.push(calls));
        self.band = counts;
        if RECORDING.with(Cell::get) {
            BAND.with(|band| band.set(Some(self.bands.len())));
        }
    }

    /// The frame is drawn: its line, the screen shown being `screen`.
    pub fn done(self, screen: &str) {
        let took = crate::host::micros().saturating_sub(self.started);
        BAND.with(|band| band.set(None));
        let counts = now();
        let drawing = counts.since(&self.before);
        let before = LAST.with(|last| self.before.since(&last.replace(counts)));
        let millis = took as f64 / 1000.0;
        if ON.with(Cell::get) {
            println!(
                "frame {screen}: drawn in {millis:.1} ms, {} bands {:?}, {drawing}; before it {before}",
                self.bands.len(),
                self.bands,
            );
        }
        if RECORDING.with(Cell::get) {
            FRAMES.with(|frames| {
                frames.borrow_mut().push(FrameRecord {
                    screen: screen.to_string(),
                    millis,
                    bands: self.bands,
                    drawing,
                    before,
                    calls: CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut())),
                });
            });
        }
    }
}

/// The storage, its calls counted while tracing.
#[derive(Debug)]
pub struct Traced<S>(pub S);

impl<S: Storage> Storage for Traced<S> {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let read = self.0.read_at(path, offset, buf);
        let n = *read.as_ref().unwrap_or(&0) as u64;
        count(|c| {
            c.reads += 1;
            c.bytes += n;
        });
        note("read", path, n);
        read
    }

    fn size(&self, path: &str) -> Result<u64, StorageError> {
        count(|c| c.sizes += 1);
        note("size", path, 0);
        self.0.size(path)
    }

    fn modified(&self, path: &str) -> Option<i64> {
        self.0.modified(path)
    }

    fn exists(&self, path: &str) -> bool {
        count(|c| c.sizes += 1);
        note("exists", path, 0);
        self.0.exists(path)
    }

    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        count(|c| c.writes += 1);
        note("write", path, 0);
        self.0.write(path, data)
    }

    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        count(|c| c.writes += 1);
        note("append", path, 0);
        self.0.append(path, data)
    }

    fn sync(&self) -> Result<(), StorageError> {
        self.0.sync()
    }

    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        count(|c| c.lists += 1);
        note("list", dir, 0);
        self.0.list(dir, visit)
    }

    fn list_from(
        &self,
        dir: &str,
        from: u64,
        visit: &mut dyn FnMut(DirEntry<'_>) -> bool,
    ) -> Result<Option<u64>, StorageError> {
        count(|c| c.lists += 1);
        note("list", dir, 0);
        self.0.list_from(dir, from, visit)
    }

    fn remove(&self, path: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        note("remove", path, 0);
        self.0.remove(path)
    }

    fn rename(&self, from: &str, to: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        note("rename", from, 0);
        self.0.rename(from, to)
    }

    fn create_dir(&self, path: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        note("create_dir", path, 0);
        self.0.create_dir(path)
    }

    fn remove_tree_step(
        &self,
        root: &str,
        keep_root: bool,
        place: u64,
    ) -> Result<paperpaw_platform::tree::TreeStep, StorageError> {
        count(|c| c.other += 1);
        note("remove_tree_step", root, place);
        self.0.remove_tree_step(root, keep_root, place)
    }

    fn remove_dir_all(&self, path: &str) -> Result<(), StorageError> {
        count(|c| c.other += 1);
        note("remove_dir_all", path, 0);
        self.0.remove_dir_all(path)
    }

    fn ignores_case(&self) -> bool {
        self.0.ignores_case()
    }

    fn same_entry(&self, a: &str, b: &str) -> Option<bool> {
        self.0.same_entry(a, b)
    }

    fn entry_identity(&self, path: &str) -> Result<Option<u64>, StorageError> {
        self.0.entry_identity(path)
    }

    fn free_space(&self) -> Option<u64> {
        self.0.free_space()
    }

    fn total_space(&self) -> Option<u64> {
        self.0.total_space()
    }

    fn local_path(&self, path: &str, with: &mut dyn FnMut(&str)) {
        self.0.local_path(path, with);
    }
}
