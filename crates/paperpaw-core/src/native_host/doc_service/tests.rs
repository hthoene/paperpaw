#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Service-level tests of `core.document` over a fake host: every input rule,
//! the lease, the state rules, the paging rules and the checks of an answer.
extern crate std;
use super::*;
use crate::abi7::{
    context::{Frame, RowsEnd},
    services_wire::{self, ServiceOp},
};
use crate::native_host::call_services::validate_reply;
use alloc::{rc::Rc, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{
    ERR_BUSY, ERR_CANCELLED, ERR_DENIED, ERR_FULL, ERR_IO, ERR_NO_MEMORY, ERR_STATE,
};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Settings};
use wire::{
    ServiceDocAt as At, ServiceDocImage as Image, ServiceDocOpen as Open, ServiceDocOption as Opt,
    ServiceDocPage as Page, ServiceDocRef as Ref, ServiceDocState as State,
};

/// The owner of the fake format plugin.
pub(crate) const FORMAT_OWNER: u64 = 99;

#[path = "tests/blocks.rs"]
mod blocks;
#[path = "tests/cover.rs"]
mod cover;
#[path = "tests/lists.rs"]
mod lists;
#[path = "tests/reader.rs"]
mod reader;
#[path = "tests/reads.rs"]
mod reads;
#[path = "tests/sides.rs"]
mod sides;

pub(crate) const OWNER: u64 = 7;
pub(crate) const FORMAT: FormatRef = FormatRef(3);

/// A bare stream of records, as a format answers.
pub(crate) fn stream(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    for record in records {
        let mut one = std::vec![0; 4096];
        let used = wire::record_encode_one(record, &mut one).unwrap();
        out.extend_from_slice(&one[..used]);
    }
    out
}
/// A counted input batch.
pub(crate) fn batch(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = std::vec![0; 4096];
    let used = wire::records_encode_counted(records, 64, 4096, &mut out).unwrap();
    out.truncate(used);
    out
}

pub(crate) struct Fake {
    table: DocumentTable,
    /// What the format answers next.
    pub(crate) answer: RefCell<Vec<u8>>,
    /// Answers (or statuses) to give, oldest first, before `answer` is used.
    pub(crate) queue: RefCell<Vec<Result<Vec<u8>, i32>>>,
    pub(crate) fails: Cell<Option<i32>>,
    /// What an open's cache step answers (`DocHost::open_cache`): a busy
    /// cache, not a busy format.
    pub(crate) cache_fails: Cell<Option<i32>>,
    /// (op, document, cursor, generation, flags, index, image_side, cover_side)
    pub(crate) calls: RefCell<Vec<[u32; 8]>>,
    pub(crate) texts: RefCell<Vec<std::string::String>>,
    pub(crate) options_seen: Cell<usize>,
    pub(crate) roots: Cell<u32>,
    pub(crate) ended: RefCell<Vec<u32>>,
    pub(crate) images_ended: RefCell<Vec<u32>>,
    /// Where the document's pictures really end (the production close path),
    /// when a test wants it; else they are only recorded.
    pub(crate) ending: Cell<Option<&'static crate::native_host::host_services::HostServices>>,
    /// The format plugin stops while the host is calling it (it crashed).
    pub(crate) stops_during_call: Cell<bool>,
    pub(crate) closed: RefCell<Vec<(u32, u32)>>,
    pub(crate) image: Cell<Option<Option<u32>>>,
    pub(crate) rows: RefCell<Vec<Vec<u8>>>,
    pub(crate) generation: Cell<u32>,
    /// The screen's long side (800 by default).
    pub(crate) long_side: Cell<u32>,
    /// The picture sides the conversion key was asked for.
    pub(crate) keyed: RefCell<Vec<(u32, u32)>>,
    pub(crate) owner: Cell<u64>,
    /// The owner of the fake format plugin.
    pub(crate) format_owner: Cell<u64>,
    /// (document, root, path) of each `reader_opened`, and the documents
    /// `reader_closed` was called for.
    pub(crate) readers: RefCell<Vec<(u32, u32, std::string::String)>>,
    pub(crate) released: RefCell<Vec<u32>>,
    /// `reader_opened` refuses with this.
    pub(crate) reader_refuses: Cell<Option<i32>>,
    /// Bit `n` set: owner `n` shows a screen.
    pub(crate) shows: Cell<u64>,
    /// The documents `picture_left_out` was called for, in order.
    pub(crate) left_out: RefCell<Vec<u32>>,
    /// The kernel's start has not returned: no cover job begins.
    pub(crate) jobs_blocked: Cell<bool>,
    /// The user's quiet time is not over (`cover_quiet`).
    pub(crate) noisy: Cell<bool>,
    /// A `cover` asked for a job while `jobs_blocked`.
    pub(crate) held_back: Cell<bool>,
    /// The cover job's marker cannot be written (`cover_job_guard`).
    pub(crate) guard_fails: Cell<bool>,
    /// The paths the marker was written for, and whether it was removed.
    pub(crate) guarded: RefCell<Vec<std::string::String>>,
}
impl Fake {
    pub(crate) fn new(limits: Limits) -> Rc<Self> {
        Rc::new(Self {
            table: DocumentTable::try_new(limits).unwrap(),
            answer: RefCell::new(Vec::new()),
            queue: RefCell::new(Vec::new()),
            fails: Cell::new(None),
            cache_fails: Cell::new(None),
            calls: RefCell::new(Vec::new()),
            texts: RefCell::new(Vec::new()),
            options_seen: Cell::new(0),
            roots: Cell::new(0b11),
            ended: RefCell::new(Vec::new()),
            images_ended: RefCell::new(Vec::new()),
            ending: Cell::new(None),
            stops_during_call: Cell::new(false),
            closed: RefCell::new(Vec::new()),
            image: Cell::new(Some(Some(41))),
            rows: RefCell::new(Vec::new()),
            generation: Cell::new(5),
            long_side: Cell::new(800),
            keyed: RefCell::new(Vec::new()),
            owner: Cell::new(OWNER),
            format_owner: Cell::new(FORMAT_OWNER),
            readers: RefCell::new(Vec::new()),
            released: RefCell::new(Vec::new()),
            reader_refuses: Cell::new(None),
            left_out: RefCell::new(Vec::new()),
            jobs_blocked: Cell::new(false),
            noisy: Cell::new(false),
            held_back: Cell::new(false),
            guard_fails: Cell::new(false),
            guarded: RefCell::new(Vec::new()),
            shows: Cell::new(0),
        })
    }
    pub(crate) fn say(&self, records: &[Record<'_>]) {
        *self.answer.borrow_mut() = stream(records);
    }
}
impl DocHost for Fake {
    fn as_dyn(&self) -> &dyn DocHost {
        self
    }
    fn table(&self) -> &DocumentTable {
        &self.table
    }
    fn screen_long_side(&self) -> u32 {
        self.long_side.get()
    }
    fn root_access(&self, root: u32) -> Result<(), i32> {
        if self.roots.get() & (1 << root) == 0 {
            Err(ERR_DENIED)
        } else {
            Ok(())
        }
    }
    fn select_format(&self, _: u32, path: &str, requested: &str) -> Result<FormatRef, i32> {
        if requested == "nobody.none" || path.ends_with(".xyz") {
            Err(ERR_NOT_FOUND)
        } else {
            Ok(FORMAT)
        }
    }
    fn format_id(&self, _: FormatRef, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        visit("fmt.epub");
        Ok(())
    }
    fn conversion_key(&self, open: &Open<'_>, _: FormatRef, _: Options<'_>) -> Result<u64, i32> {
        self.keyed
            .borrow_mut()
            .push((open.image_side, open.cover_side));
        Ok(0xc0de)
    }
    fn open_cache(&self, _call: &FormatCall<'_>) -> Result<(), i32> {
        self.cache_fails.get().map_or(Ok(()), Err)
    }
    fn call_format(&self, call: &FormatCall<'_>, out: &mut [u8]) -> Result<usize, i32> {
        let mut options = 0;
        call.options.each(|_| options += 1);
        self.options_seen.set(options);
        self.calls.borrow_mut().push([
            call.op,
            call.document,
            call.cursor,
            call.generation,
            call.flags,
            call.index,
            call.image_side,
            call.cover_side,
        ]);
        self.texts.borrow_mut().push(call.text.into());
        if self.stops_during_call.get() {
            format_stopped(self, FORMAT).unwrap_or(0);
        }
        if let Some(error) = self.fails.get() {
            return Err(error);
        }
        let queued = {
            let mut queue = self.queue.borrow_mut();
            (!queue.is_empty()).then(|| queue.remove(0))
        };
        if let Some(queued) = queued {
            let bytes = queued?;
            out.get_mut(..bytes.len())
                .ok_or(ERR_NO_SPACE)?
                .copy_from_slice(&bytes);
            return Ok(bytes.len());
        }
        let answer = self.answer.borrow();
        out.get_mut(..answer.len())
            .ok_or(ERR_NO_SPACE)?
            .copy_from_slice(&answer);
        Ok(answer.len())
    }
    fn close_format(&self, document: &Document) {
        self.ended.borrow_mut().push(document.id);
    }
    fn end_images(&self, document: &Document) {
        self.images_ended.borrow_mut().push(document.id);
        if let Some(hosts) = self.ending.get() {
            // As the production host does: the document's pictures end now.
            let _ = hosts.images().document_closed(document.id);
        }
    }
    fn document_closed(&self, document: &Document, reason: u32) {
        self.closed.borrow_mut().push((document.id, reason));
    }
    fn reader_opened(&self, document: u32, root: u32, path: &str) -> Result<(), i32> {
        if let Some(error) = self.reader_refuses.get() {
            return Err(error);
        }
        self.readers
            .borrow_mut()
            .push((document, root, path.into()));
        Ok(())
    }
    fn reader_closed(&self, document: u32) {
        self.released.borrow_mut().push(document);
    }
    fn shows_screen(&self, owner: u64) -> bool {
        self.shows.get() & (1 << owner) != 0
    }
    fn cover_jobs_allowed(&self) -> bool {
        !self.jobs_blocked.get()
    }
    fn cover_quiet(&self) -> bool {
        !self.noisy.get()
    }
    fn cover_at(&self, _path: &str) -> crate::native_host::doc_host::CoverAt {
        use crate::native_host::doc_host::CoverAt;
        // The host's table knows an unmade book; the fake's quiet time stands in.
        if self.noisy.get() {
            CoverAt::Unmade
        } else {
            CoverAt::Made
        }
    }
    fn cover_job_guard(&self, path: &str) -> Result<(), i32> {
        if self.guard_fails.get() {
            return Err(ERR_IO);
        }
        self.guarded.borrow_mut().push(path.into());
        Ok(())
    }
    fn cover_job_held_back(&self) {
        self.held_back.set(true);
    }
    fn picture_left_out(&self, document: u32) {
        self.left_out.borrow_mut().push(document);
    }
    fn map_image(&self, _: &Document, _: &Image, in_block: bool) -> Result<Option<u32>, i32> {
        match self.image.get() {
            Some(Some(0)) if !in_block => Err(ERR_BUSY),
            other => Ok(other.flatten()),
        }
    }
    fn formats_of_owner(&self, owner: u64, visit: &mut dyn FnMut(FormatRef)) {
        // The fake's format plugin is owner 99.
        if owner == self.format_owner.get() {
            visit(FORMAT);
        }
    }
    fn formats(
        &self,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.page(cursor, generation, |bytes| {
            let Record::ServiceDocFormat(row) = wire::record_decode_one(bytes).unwrap() else {
                unreachable!()
            };
            visit(Record::ServiceDocFormat(row))
        })
    }
    fn readers(
        &self,
        scope: &str,
        cursor: u32,
        generation: u32,
        visit: &mut dyn FnMut(Record<'_>) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        self.texts.borrow_mut().push(scope.into());
        self.page(cursor, generation, |bytes| {
            let Record::ServiceDocReader(row) = wire::record_decode_one(bytes).unwrap() else {
                unreachable!()
            };
            visit(Record::ServiceDocReader(row))
        })
    }
    fn format_for(
        &self,
        root: u32,
        path: &str,
        sniff: bool,
        visit: &mut dyn FnMut(Record<'_>),
    ) -> Result<(), i32> {
        self.texts.borrow_mut().push(path.into());
        self.calls
            .borrow_mut()
            .push([root, u32::from(sniff), 0, 0, 0, 0, 0, 0]);
        if path.ends_with(".xyz") {
            return Err(ERR_NOT_FOUND);
        }
        visit(Record::ServiceDocFormat(wire::ServiceDocFormat {
            id: "fmt.epub",
            title: "Epub",
            extensions: "epub",
            flags: 0,
            priority: -2,
        }));
        Ok(())
    }
}
impl Fake {
    fn page(
        &self,
        cursor: u32,
        generation: u32,
        mut visit: impl FnMut(&[u8]) -> ControlFlow<()>,
    ) -> Result<RowsEnd, i32> {
        let rows = self.rows.borrow();
        let total = u32::try_from(rows.len()).unwrap();
        if generation != 0 && generation != self.generation.get() {
            return Err(ERR_STATE);
        }
        let mut next = cursor.min(total);
        for row in rows.iter().skip(next as usize) {
            if visit(row).is_break() {
                break;
            }
            next += 1;
        }
        Ok(RowsEnd {
            total,
            generation: self.generation.get(),
            next,
        })
    }
}

pub(crate) struct View<'a> {
    env: &'a Env<'a>,
    hosts: crate::native_host::host_services::HostServices,
    fake: Rc<Fake>,
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
    /// The backend as the dispatcher calls it, for the fake's current owner.
    pub(crate) fn run(&self, op: &ServiceOp, input: &[u8], out: &mut [u8]) -> Result<usize, i32> {
        let caller = Caller {
            owner: self.fake.owner.get(),
            plugin: "opener",
            features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
            hosts: &self.hosts,
        };
        backend(self, &caller, op, input, out)
    }
}

/// Run `body` with a frame over `fake`.
pub(crate) fn with<R>(fake: &Rc<Fake>, body: impl FnOnce(&View<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let host: Rc<dyn DocHost> = fake.clone();
    let hosts = crate::native_host::host_services::HostServices::new();
    hosts.wire_documents(host).unwrap();
    body(&View {
        env: &env,
        hosts,
        fake: fake.clone(),
    })
}

pub(crate) fn schema_op(id: u32) -> &'static ServiceOp {
    services_wire::service_schema("core.document", 1)
        .unwrap()
        .op(id)
        .unwrap()
}

/// One call: encode `records`, run the backend into the op's scratch, check the
/// reply as the dispatcher does, return the reply bytes.
pub(crate) fn call(fake: &Rc<Fake>, id: u32, records: &[Record<'_>]) -> Result<Vec<u8>, i32> {
    let op = schema_op(id);
    let mut input = std::vec![0; 4096];
    let used = wire::records_encode_counted(
        records,
        usize::from(op.input_rows),
        op.max_input,
        &mut input,
    )?;
    call_raw(fake, id, &input[..used])
}
pub(crate) fn call_raw(fake: &Rc<Fake>, id: u32, input: &[u8]) -> Result<Vec<u8>, i32> {
    let op = schema_op(id);
    let mut out = std::vec![0; op.max_reply];
    let length = with(fake, |frame| frame.run(op, input, &mut out))?;
    out.truncate(length);
    validate_reply(&out, op)?;
    Ok(out)
}

pub(crate) fn open_record() -> Open<'static> {
    Open {
        root: 1,
        path: "books/a.epub",
        format: "",
        flags: 0b101,
        image_side: 0,
        cover_side: 0,
        css_root: 0,
        css_path: "",
    }
}
pub(crate) fn state_answer(state: u32, status: i32) -> Vec<u8> {
    stream(&[Record::ServiceDocState(State {
        document: 99,
        state,
        progress_ppm: 250_000,
        status,
        flags: 0b10,
        conversion: 1,
        format: "other.format",
    })])
}
pub(crate) fn state_of(bytes: &[u8]) -> State<'_> {
    let Record::ServiceDocState(state) = wire::record_decode_one(bytes).unwrap() else {
        panic!("not a doc_state")
    };
    state
}
/// Open one document as the default owner and return its id.
pub(crate) fn opened(fake: &Rc<Fake>) -> u32 {
    *fake.answer.borrow_mut() = state_answer(0, 0);
    let reply = call(fake, 1, &[Record::ServiceDocOpen(open_record())]).unwrap();
    state_of(&reply).document
}
pub(crate) fn doc_ref(document: u32) -> Record<'static> {
    Record::ServiceDocRef(Ref { document })
}

#[test]
fn open_replies_with_the_hosts_own_fields_not_the_formats() {
    let fake = Fake::new(Limits::X4);
    fake.say(&[Record::ServiceDocState(State {
        document: 99,
        state: 0,
        progress_ppm: 0,
        status: 0,
        flags: 0b10,
        conversion: 1,
        format: "other.format",
    })]);
    let reply = call(&fake, 1, &[Record::ServiceDocOpen(open_record())]).unwrap();
    let state = state_of(&reply);
    assert_eq!(
        (state.document, state.format, state.conversion),
        (1, "fmt.epub", 0xc0de)
    );
    assert_eq!((state.state, state.flags), (READY_STATE, 0b10));
    // The format was called with op 2, this id, the open flags, and the
    // default picture sizes (1,600 and the larger of 300 and 3/8 of 800).
    assert_eq!(
        fake.calls.borrow()[0],
        [fmt::OPEN, 1, 0, 0, 0b101, 0, 1600, 300]
    );
}
const READY_STATE: u32 = 0;

#[test]
fn open_forwards_all_twelve_options_whole() {
    let fake = Fake::new(Limits::X4);
    fake.say(&[Record::ServiceDocState(State {
        document: 0,
        state: 0,
        progress_ppm: 0,
        status: 0,
        flags: 0,
        conversion: 0,
        format: "",
    })]);
    let mut records = std::vec![Record::ServiceDocOpen(open_record())];
    for _ in 0..12 {
        records.push(Record::ServiceDocOption(Opt {
            key: "reader.size",
            value: "18",
        }));
    }
    call(&fake, 1, &records).unwrap();
    assert_eq!(fake.options_seen.get(), 12);
    // A 13th option is the generic row cap's ERR_NO_SPACE, before any host work.
    records.push(Record::ServiceDocOption(Opt {
        key: "reader.size",
        value: "18",
    }));
    assert_eq!(args::parse_open(&batch(&records)).err(), Some(ERR_NO_SPACE));
}

#[test]
fn open_refuses_a_missing_repeated_or_out_of_order_record() {
    let fake = Fake::new(Limits::X4);
    let open = || Record::ServiceDocOpen(open_record());
    let option = || {
        Record::ServiceDocOption(Opt {
            key: "reader.size",
            value: "18",
        })
    };
    for records in [
        std::vec![],
        std::vec![option()],
        std::vec![option(), open()],
        std::vec![open(), open()],
        std::vec![open(), option(), open()],
        std::vec![Record::ServiceDocRef(Ref { document: 1 })],
    ] {
        let input = batch(&records);
        assert_eq!(
            call_raw(&fake, 1, &input),
            Err(ERR_INVALID),
            "{}",
            records.len()
        );
    }
    assert!(fake.calls.borrow().is_empty());
    assert_eq!(fake.table.len(), 0);
}

#[test]
fn open_refuses_bad_values_before_any_host_work() {
    let fake = Fake::new(Limits::X4);
    let with_open = |edit: &dyn Fn(&mut Open<'static>)| {
        let mut open = open_record();
        edit(&mut open);
        call(&fake, 1, &[Record::ServiceDocOpen(open)])
    };
    assert_eq!(with_open(&|o| o.root = 2), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.css_root = 2), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.flags = 0b1000), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.path = ""), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.path = "a/../b.epub"), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.path = "/abs.epub"), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.css_path = "a/./b.css"), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.format = "Bad Id"), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.format = "nodot"), Err(ERR_INVALID));
    // The longest side of a page picture is at most 1,600 (0 is 1,600).
    assert_eq!(with_open(&|o| o.image_side = 1601), Err(ERR_INVALID));
    assert_eq!(with_open(&|o| o.cover_side = 801), Err(ERR_INVALID));
    assert!(
        with_open(&|o| o.cover_side = 800).is_err(),
        "the format has no answer yet"
    );
    assert_eq!(fake.table.len(), 0);
    let option = Record::ServiceDocOption(Opt {
        key: "NOT AN ID",
        value: "",
    });
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(open_record()), option]),
        Err(ERR_INVALID)
    );
}

#[test]
fn open_checks_the_roots_and_the_format_first() {
    let fake = Fake::new(Limits::X4);
    fake.roots.set(0b01);
    fake.say(&[]);
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(open_record())]),
        Err(ERR_DENIED)
    );
    let mut css = open_record();
    css.root = 0;
    css.css_root = 1;
    css.css_path = "my.css";
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(css)]),
        Err(ERR_DENIED)
    );
    fake.roots.set(0b11);
    let mut unknown = open_record();
    unknown.path = "books/a.xyz";
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(unknown)]),
        Err(ERR_NOT_FOUND)
    );
    let mut asked = open_record();
    asked.format = "nobody.none";
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(asked)]),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!((fake.table.len(), fake.calls.borrow().len()), (0, 0));
}

#[test]
fn a_failed_or_malformed_format_open_leaves_no_id_behind() {
    let fake = Fake::new(Limits::X4);
    fake.fails.set(Some(ERR_IO));
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(open_record())]),
        Err(ERR_IO)
    );
    assert_eq!((fake.table.len(), fake.ended.borrow().len()), (0, 1));
    fake.fails.set(None);
    let bad_states = [
        stream(&[]),
        [state_answer(0, 0), state_answer(0, 0)].concat(),
        state_answer(3, 0),
        state_answer(2, 0),
        state_answer(0, -2),
        stream(&[Record::ServiceDocRef(Ref { document: 1 })]),
        stream(&[Record::ServiceDocState(State {
            document: 0,
            state: 0,
            progress_ppm: 1_000_001,
            status: 0,
            flags: 0,
            conversion: 0,
            format: "",
        })]),
        stream(&[Record::ServiceDocState(State {
            document: 0,
            state: 0,
            progress_ppm: 0,
            status: 0,
            flags: 0x80,
            conversion: 0,
            format: "",
        })]),
    ];
    for answer in bad_states {
        *fake.answer.borrow_mut() = answer;
        assert_eq!(
            call(&fake, 1, &[Record::ServiceDocOpen(open_record())]),
            Err(ERR_INVALID)
        );
        assert_eq!(fake.table.len(), 0);
    }
}

#[test]
fn a_failed_book_keeps_its_id_so_the_opener_reads_the_status() {
    let fake = Fake::new(Limits::X4);
    *fake.answer.borrow_mut() = state_answer(2, ERR_IO);
    let reply = call(&fake, 1, &[Record::ServiceDocOpen(open_record())]).unwrap();
    let state = state_of(&reply);
    assert_eq!((state.state, state.status), (2, ERR_IO));
    let id = state.document;
    let again = call(&fake, 2, &[doc_ref(id)]).unwrap();
    assert_eq!(state_of(&again).status, ERR_IO);
    assert_eq!(call(&fake, 3, &[doc_ref(id)]), Err(ERR_STATE));
    call(&fake, 18, &[doc_ref(id)]).unwrap();
    assert_eq!(call(&fake, 2, &[doc_ref(id)]), Err(ERR_NOT_FOUND));
}

#[test]
fn a_preparing_book_answers_state_and_close_and_is_busy_for_the_rest() {
    let fake = Fake::new(Limits::X4);
    *fake.answer.borrow_mut() = state_answer(1, 0);
    let id = state_of(&call(&fake, 1, &[Record::ServiceDocOpen(open_record())]).unwrap()).document;
    assert_eq!(state_of(&call(&fake, 2, &[doc_ref(id)]).unwrap()).state, 1);
    for op in 3..=17 {
        let input = reads::input_for(op, id);
        assert_eq!(call_raw(&fake, op, &input), Err(ERR_BUSY), "op {op}");
    }
    assert_eq!(
        fake.calls.borrow().len(),
        1,
        "only the open reached the format"
    );
    call(&fake, 18, &[doc_ref(id)]).unwrap();
}

#[test]
fn limits_ids_and_leases() {
    let fake = Fake::new(Limits::X4);
    let first = opened(&fake);
    // On the X4 (two slots) an opener keeps off the last one, so that the
    // reader's shown book can always open: no background index takes both.
    *fake.answer.borrow_mut() = state_answer(0, 0);
    let open = [Record::ServiceDocOpen(open_record())];
    assert_eq!(call(&fake, 1, &open), Err(ERR_FULL), "no id spent");
    fake.owner.set(8);
    let second = opened(&fake);
    assert_eq!((first, second), (1, 2));
    // The device limit: a third opener is full too, no id spent.
    fake.owner.set(9);
    assert_eq!(call(&fake, 1, &open), Err(ERR_FULL));
    fake.owner.set(8);
    // Another opener's document is denied, an unknown one is not found.
    assert_eq!(call(&fake, 2, &[doc_ref(first)]), Err(ERR_DENIED));
    assert_eq!(call(&fake, 18, &[doc_ref(first)]), Err(ERR_DENIED));
    assert_eq!(call(&fake, 2, &[doc_ref(77)]), Err(ERR_NOT_FOUND));
    assert_eq!(call(&fake, 2, &[doc_ref(0)]), Err(ERR_NOT_FOUND));
    fake.owner.set(OWNER);
    // Closing frees the slot; ids are never reused.
    assert_eq!(
        call(&fake, 18, &[doc_ref(first)]).unwrap(),
        Vec::<u8>::new()
    );
    assert_eq!(*fake.ended.borrow(), [first]);
    assert_eq!(call(&fake, 18, &[doc_ref(first)]), Err(ERR_NOT_FOUND));
    assert_eq!(opened(&fake), 3);
}

#[test]
fn per_owner_limit_holds_where_the_device_has_room() {
    let fake = Fake::new(Limits::LARGE);
    opened(&fake);
    opened(&fake);
    *fake.answer.borrow_mut() = state_answer(0, 0);
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(open_record())]),
        Err(ERR_FULL)
    );
    fake.owner.set(9);
    assert_eq!(opened(&fake), 3);
}

#[test]
fn a_document_the_host_closed_is_cancelled_until_its_opener_closes_it() {
    let fake = Fake::new(Limits::X4);
    let id = opened(&fake);
    fake.table.cancel(id, 9).unwrap();
    assert_eq!(call(&fake, 2, &[doc_ref(id)]), Err(ERR_CANCELLED));
    assert_eq!(call(&fake, 3, &[doc_ref(id)]), Err(ERR_CANCELLED));
    assert_eq!(fake.table.lease(OWNER, id), Err(ERR_CANCELLED));
    call(&fake, 18, &[doc_ref(id)]).unwrap();
    assert_eq!(fake.table.close_owner(OWNER, |_| ()), Ok(0));
}

#[test]
fn a_stopped_opener_loses_all_its_documents() {
    let fake = Fake::new(Limits::LARGE);
    let ids = [opened(&fake), opened(&fake)];
    assert_eq!(opener_stopped(&*fake, OWNER), Ok(ids.len()));
    assert_eq!(fake.table.len(), 0);
    // Each document's format was told to close it, once, after the table was
    // released (the host's callback could use the table).
    assert_eq!(*fake.ended.borrow(), ids);
}

#[test]
fn a_stopped_format_ends_its_documents_and_their_openers_hear_it() {
    let fake = Fake::new(Limits::LARGE);
    let a = opened(&fake);
    fake.owner.set(9);
    let b = opened(&fake);
    fake.owner.set(OWNER);
    // The other format's documents stay.
    assert_eq!(format_stopped(&*fake, FormatRef(FORMAT.0 + 1)), Ok(0));
    assert_eq!(format_stopped(&*fake, FORMAT), Ok(2));
    assert_eq!(*fake.closed.borrow(), [(a, 1), (b, 1)]);
    // Told once; the format is not called again; the opener closes the id.
    assert_eq!(format_stopped(&*fake, FORMAT), Ok(0));
    assert!(fake.ended.borrow().is_empty());
    assert_eq!(call(&fake, 2, &[doc_ref(a)]), Err(ERR_CANCELLED));
    call(&fake, 18, &[doc_ref(a)]).unwrap();
    assert!(
        fake.ended.borrow().is_empty(),
        "closing it does not call the stopped format"
    );
    // A host-closed document of a live format (here: the card went) still has
    // its format told to close it.
    let c = opened(&fake);
    fake.table.cancel(c, 2).unwrap();
    call(&fake, 18, &[doc_ref(c)]).unwrap();
    assert_eq!(*fake.ended.borrow(), [c]);
}

#[test]
fn the_table_allocates_once_and_ids_run_out_without_wrapping() {
    // The room is made at the first open: a table too large for the heap
    // says so there, and keeps nothing.
    let huge = DocumentTable::try_new(Limits {
        per_owner: 1,
        device: usize::MAX / 8,
    })
    .unwrap();
    assert_eq!(huge.capacity(), 0);
    assert_eq!(
        huge.open(1, true, FORMAT, 0, 0, &|_| true).err(),
        Some(ERR_NO_MEMORY)
    );
    assert_eq!(huge.capacity(), 0);
    let fake = Fake::new(Limits::X4);
    // A device that opens nothing keeps no room.
    assert_eq!(fake.table.capacity(), 0);
    let id = opened(&fake);
    let capacity = fake.table.capacity();
    assert!(capacity >= 2);
    call(&fake, 18, &[doc_ref(id)]).unwrap();
    opened(&fake);
    assert_eq!(
        fake.table.capacity(),
        capacity,
        "open and close never grow the table"
    );
    fake.table.set_next_id(u32::MAX);
    *fake.answer.borrow_mut() = state_answer(0, 0);
    fake.owner.set(8);
    assert_eq!(
        call(&fake, 1, &[Record::ServiceDocOpen(open_record())]),
        Err(ERR_NO_SPACE)
    );
    assert_eq!(fake.table.len(), 1);
}

#[test]
fn a_frame_without_the_host_or_a_caller_refuses_the_family() {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let fake = Fake::new(Limits::X4);
    let view = View {
        env: &env,
        hosts: crate::native_host::host_services::HostServices::new(),
        fake: fake.clone(),
    };
    let op = schema_op(2);
    let input = batch(&[doc_ref(1)]);
    let mut out = std::vec![0; op.max_reply];
    assert_eq!(view.run(op, &input, &mut out), Err(ERR_UNSUPPORTED));
    let _ = Image {
        image: 0,
        index: 0,
        width: 0,
        height: 0,
        flags: 0,
    };
    let _ = (
        At {
            document: 0,
            section: 0,
            offset: 0,
            insert: 0,
            cursor: 0,
            generation: 0,
        },
        Page {
            document: 0,
            scope: "",
            cursor: 0,
            generation: 0,
        },
    );
}
