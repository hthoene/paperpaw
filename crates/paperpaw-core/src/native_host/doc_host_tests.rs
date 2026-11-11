//! The production document host: formats and readers from the declarations,
//! the user's choice over priority, a book opened through the format's own
//! instance, the reader hold, a format that stops, and the refused allocations.
#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
extern crate std;
use super::*;
use crate::abi7::{
    context::registry_views::{Asker, RegistryView, Row as RegRow},
    services_wire,
};
use crate::native_host::{
    admitted_features::AdmittedFeatures,
    call_services::validate_reply,
    declaration_store::{Allocations, DeclarationStore, ImageOwner, PendingDeclarations},
    doc_service::{self, DocHost},
    host_services::HostServices,
    registrations::{REGISTRATION_MAX_BYTES, records_encode},
    ui_guard::{Row, Rows, UiStackView},
};
use alloc::{string::ToString, vec};
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_IO, ERR_NO_SPACE};
use paperpaw_native::metadata::Feature;
use paperpaw_platform::Instant;
use paperpaw_sdk::{Env, Id, Settings};

const OPENER: u64 = 7;
const EPUB: u64 = 11;
const FEATURES: &[Feature<'static>] = &[Feature {
    key: "uses.storage",
    revision: 1,
}];

struct Plugin {
    id: &'static str,
    host: u64,
    live: Cell<bool>,
}
impl ImageOwner for Plugin {
    fn plugin_id(&self) -> &str {
        self.id
    }
    fn generation(&self) -> u32 {
        u32::from(self.live.get())
    }
    fn owner_id(&self) -> Result<u64, i32> {
        Ok(self.host)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(self.live.get().then_some(&[][..]));
    }
    fn invalid_declaration(&self, _: i32) {}
}

fn format<'a>(name: &'a str, extensions: &'a str, flags: u32, priority: i32) -> Record<'a> {
    Record::FormatSpec(wire::FormatSpec {
        name,
        title: "format.title",
        extensions,
        flags,
        priority,
        token: 5,
    })
}

/// The frame of a call: the declarations, a generation, a format that answers.
struct World<'a> {
    env: &'a Env<'a>,
    settings: Cell<&'static Settings>,
    store: RefCell<DeclarationStore>,
    plugins: RefCell<Vec<Rc<Plugin>>>,
    generation: Cell<u32>,
    walks: Cell<u32>,
    /// The allocation of the next backend call to refuse (0: none).
    fail: Cell<u32>,
    /// The length of the bytes each sniff was given.
    sniffed: RefCell<Vec<usize>>,
    /// The card, as the host reads and the format's cache is on it.
    card: Rc<paperpaw_sdk::MemoryStorage>,
    /// Whether the format writes a page of its cache in every call.
    makes_cache: Cell<bool>,
    /// The renames the card was asked for (the host's stamp is one).
    renames: Rc<Cell<u32>>,
    /// The version of every plugin, as the registry says it.
    version: Cell<&'static str>,
    /// What the format's call is for, as its slots see it: (book path, kind,
    /// opened file, document, CSS card path), one per call.
    scoped: RefCell<Vec<Scoped>>,
    /// The format plugins (by host owner) that answer a sniff with yes.
    sniffs: RefCell<Vec<u64>>,
    /// What the format answers to the next call.
    reply: RefCell<Result<Vec<u8>, i32>>,
    /// (owner, token, the call's fmt op) of each call made to a format.
    calls: RefCell<Vec<(u64, u32, u32)>>,
    hosts: HostServices,
    state: Rc<DocState>,
}

impl RegistryView for World<'_> {
    fn generation(&self) -> u32 {
        self.generation.get()
    }
    fn rows(
        &self,
        _: u32,
        _: &str,
        _: u32,
        _: &Asker<'_>,
        _: &mut dyn FnMut(&dyn RegRow) -> ControlFlow<()>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn detail(
        &self,
        op: u32,
        key: &str,
        _: &Asker<'_>,
        visit: &mut dyn FnMut(&dyn RegRow) -> Result<(), i32>,
    ) -> Result<(), i32> {
        // `plugin`: every plugin is at the version the test says.
        if op != 6 {
            return Err(ERR_UNSUPPORTED);
        }
        visit(&Record::ServiceRegPlugin(wire::ServiceRegPlugin {
            id: key,
            name: key,
            version: self.version.get(),
            kind: 2,
            state: 0,
            flags: 0,
            problem: 0,
            problem_arg: 0,
            update: "",
        }))
    }
}

impl Frame for World<'_> {
    fn settings(&self) -> &Settings {
        self.settings.get()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn registry(&self) -> Result<&dyn RegistryView, i32> {
        Ok(self)
    }
    fn with_declarations(&self, visit: &mut dyn FnMut(&dyn core::any::Any)) -> Result<(), i32> {
        self.walks.set(self.walks.get() + 1);
        visit(&*self.store.borrow());
        Ok(())
    }
    fn with_stack(&self, visit: &mut dyn FnMut(&dyn UiStackView)) -> Result<(), i32> {
        visit(&Rows(&[Row {
            instance: 1,
            owner: OPENER,
            overlay: false,
            guards: false,
            secret: false,
            host: false,
            guard_dialog: false,
        }]));
        Ok(())
    }
    fn plugin_name(&self, plugin: &str, visit: &mut dyn FnMut(&str)) -> Result<(), i32> {
        visit(&std::format!("Name of {plugin}"));
        Ok(())
    }
    fn call_native(
        &self,
        owner: u64,
        token: u32,
        input: &[u8],
        reply: &mut [u8],
        accept: &mut dyn FnMut(&[u8]) -> Result<(), i32>,
    ) -> Result<usize, i32> {
        // The fixture's own allocations are never the ones refused.
        let _unarmed = Unarmed::new();
        let Record::StageCall(call) = wire::records_decode_single(input).unwrap() else {
            panic!("not a stage.call")
        };
        let (op, arg) = match local::input_decode(call.payload)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
        {
            local::Record::FmtCall(c) => (c.op, c.arg.len()),
            other => panic!("{other:?}"),
        };
        self.calls.borrow_mut().push((owner, token, op));
        self.hosts.format_scope().current(owner, &mut |book| {
            self.scoped.borrow_mut().push((
                book.path.into(),
                book.kind,
                book.opened.into(),
                book.document,
                book.css.into(),
                book.cache.into(),
            ));
            // A format that makes its cache: a page of it in root 6.
            if self.makes_cache.get() && !book.cache.is_empty() {
                let page = std::format!("{}/pages/1", book.cache);
                self.card.write(&page, b"page").unwrap();
            }
        });
        if op == 1 {
            self.sniffed.borrow_mut().push(arg);
        }
        if op == 1 {
            // A sniff: `fmt.yes`, or no record (not mine).
            let mut bytes = vec![0_u8; 64];
            let records: &[local::Record<'_>] = if self.sniffs.borrow().contains(&owner) {
                &[local::Record::FmtYes(local::FmtYes::new(0))]
            } else {
                &[]
            };
            let n = local::reply_encode(records, &mut bytes).unwrap();
            reply[..n].copy_from_slice(&bytes[..n]);
            accept(&reply[..n])?;
            return Ok(n);
        }
        let answer = self.reply.borrow().clone()?;
        reply[..answer.len()].copy_from_slice(&answer);
        accept(&reply[..answer.len()])?;
        Ok(answer.len())
    }
}

/// The card of the fixture: its own allocations are never the ones refused.
struct Card(Rc<paperpaw_sdk::MemoryStorage>, Rc<Cell<u32>>);
impl Storage for Card {
    fn read_at(
        &self,
        p: &str,
        o: u64,
        b: &mut [u8],
    ) -> Result<usize, paperpaw_platform::StorageError> {
        let _unarmed = Unarmed::new();
        self.0.read_at(p, o, b)
    }
    fn size(&self, p: &str) -> Result<u64, paperpaw_platform::StorageError> {
        let _unarmed = Unarmed::new();
        self.0.size(p)
    }
    fn write(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        let _unarmed = Unarmed::new();
        self.0.write(p, d)
    }
    fn append(&self, p: &str, d: &[u8]) -> Result<(), paperpaw_platform::StorageError> {
        let _unarmed = Unarmed::new();
        self.0.append(p, d)
    }
    fn list(
        &self,
        d: &str,
        v: &mut dyn FnMut(paperpaw_platform::DirEntry<'_>),
    ) -> Result<(), paperpaw_platform::StorageError> {
        let _unarmed = Unarmed::new();
        self.0.list(d, v)
    }
    fn remove(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
        let _unarmed = Unarmed::new();
        self.0.remove(p)
    }
    fn rename(&self, f: &str, t: &str) -> Result<(), paperpaw_platform::StorageError> {
        self.1.set(self.1.get() + 1);
        let _unarmed = Unarmed::new();
        self.0.rename(f, t)
    }
    fn create_dir(&self, p: &str) -> Result<(), paperpaw_platform::StorageError> {
        let _unarmed = Unarmed::new();
        self.0.create_dir(p)
    }
    fn sync(&self) -> Result<(), paperpaw_platform::StorageError> {
        self.0.sync()
    }
}

/// What a format call is for, as its slots see it.
type Scoped = (
    std::string::String,
    BookKind,
    std::string::String,
    u32,
    std::string::String,
    std::string::String,
);

/// Puts the allocation failure of the test aside while the fixture works,
/// and back as it was.
struct Unarmed(u32);
impl Unarmed {
    fn new() -> Self {
        let armed = crate::native_host::shell::tests::FAIL_NTH.with(Cell::get);
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(0));
        Self(armed)
    }
}
impl Drop for Unarmed {
    fn drop(&mut self) {
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(self.0));
    }
}

impl World<'_> {
    fn install(&self, id: &'static str, host: u64, records: &[Record<'_>]) {
        let plugin = Rc::new(Plugin {
            id,
            host,
            live: Cell::new(true),
        });
        let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
        let len = records_encode(records, &mut bytes).unwrap();
        bytes.truncate(len);
        let mut allocations = Allocations::default();
        let mut pending = PendingDeclarations::new(Id::try_parse(id).unwrap(), plugin.clone());
        pending.stage_batch(&bytes, &mut allocations).unwrap();
        self.store
            .borrow_mut()
            .prepare(pending, &mut allocations)
            .unwrap()
            .commit();
        self.plugins.borrow_mut().push(plugin);
        self.generation.set(self.generation.get() + 1);
    }

    /// The user's `core.formats.txt`: a fresh settings value (leaked: a test).
    fn choose(&self, wanted: &str) {
        let mut settings = Settings::new();
        let group = paperpaw_sdk::SettingGroupSpec::new(Id::new_static("core.group"), "g");
        settings.add_group(group).unwrap();
        settings
            .define(paperpaw_sdk::SettingSpec::text(
                "core.formats.txt",
                "t",
                "",
                Id::new_static("core.group"),
            ))
            .unwrap();
        settings
            .set(
                "core.formats.txt",
                paperpaw_sdk::SettingValue::Text(wanted.to_string().into()),
            )
            .unwrap();
        self.settings
            .set(std::boxed::Box::leak(std::boxed::Box::new(settings)));
    }

    fn stop(&self, id: &str) {
        for plugin in self.plugins.borrow().iter().filter(|p| p.id == id) {
            plugin.live.set(false);
        }
        self.generation.set(self.generation.get() + 1);
    }

    fn call(
        &self,
        features: AdmittedFeatures,
        id: u32,
        records: &[Record<'_>],
    ) -> Result<Vec<u8>, i32> {
        let schema = services_wire::service_schema("core.document", 1).unwrap();
        let op = schema.op(id).unwrap();
        let mut input = vec![0; 4096];
        let used = wire::records_encode_counted(
            records,
            usize::from(op.input_rows),
            op.max_input,
            &mut input,
        )?;
        self.call_raw(features, op, &input[..used])
    }

    fn call_raw(
        &self,
        features: AdmittedFeatures,
        op: &services_wire::ServiceOp,
        input: &[u8],
    ) -> Result<Vec<u8>, i32> {
        let caller = Caller {
            owner: OPENER,
            plugin: "opener",
            features,
            hosts: &self.hosts,
        };
        let mut out = vec![0; op.max_reply];
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(self.fail.get()));
        let answer = doc_service::backend(self, &caller, op, input, &mut out);
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(0));
        let length = answer?;
        out.truncate(length);
        validate_reply(&out, op)?;
        Ok(out)
    }

    fn open(&self, root: u32, path: &str) -> Result<u32, i32> {
        self.open_with_css(root, path, 0, "")
    }

    fn open_with_css(
        &self,
        root: u32,
        path: &str,
        css_root: u32,
        css_path: &str,
    ) -> Result<u32, i32> {
        let features = if root == 1 {
            AdmittedFeatures::test(1, FEATURES)
        } else {
            AdmittedFeatures::NONE
        };
        let reply = self.call(
            features,
            1,
            &[Record::ServiceDocOpen(wire::ServiceDocOpen {
                root,
                path,
                format: "",
                flags: 0,
                image_side: 0,
                cover_side: 0,
                css_root,
                css_path,
            })],
        )?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceDocState(state) => Ok(state.document),
            other => panic!("{other:?}"),
        }
    }

    fn format_for(&self, path: &str) -> Result<std::string::String, i32> {
        let reply = self.call(
            AdmittedFeatures::NONE,
            20,
            &[Record::ServiceDocPath(wire::ServiceDocPath {
                root: 0,
                path,
                flags: 0,
            })],
        )?;
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceDocFormat(f) => Ok(f.id.to_string()),
            other => panic!("{other:?}"),
        }
    }

    fn list(&self, op: u32) -> Result<Vec<(std::string::String, std::string::String)>, i32> {
        let reply = self.call(
            AdmittedFeatures::NONE,
            op,
            &[Record::ServiceDocPage(wire::ServiceDocPage {
                document: 0,
                scope: "",
                cursor: 0,
                generation: 0,
            })],
        )?;
        let mut rows = Vec::new();
        crate::native_host::stage_wire::visit_stream(&reply, |_, framed| {
            match wire::record_decode_one(framed).unwrap() {
                Record::ServiceDocFormat(f) => rows.push((f.id.into(), f.title.into())),
                Record::ServiceDocReader(r) => rows.push((r.id.into(), r.title.into())),
                Record::ServiceEnd(_) => {}
                other => panic!("{other:?}"),
            }
            Ok(())
        })
        .unwrap();
        Ok(rows)
    }
}

fn state_reply() -> Vec<u8> {
    let mut bytes = vec![0_u8; 3840];
    let n = local::reply_encode(
        &[local::Record::FmtState(local::FmtState::new(
            0,
            0,
            0,
            0,
            0,
            5,
            "epub.epub",
        ))],
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(n);
    bytes
}

fn world<R>(body: impl FnOnce(&World<'_>) -> R) -> R {
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(Instant(0));
    let hosts = HostServices::new();
    let renames = Rc::new(Cell::new(0));
    let card = Rc::new(
        paperpaw_sdk::MemoryStorage::new()
            .with("mystery", b"PK\x03\x04 epub?".to_vec())
            .with("odd.zzz", b"%PDF-1.7".to_vec())
            .with("big", vec![7_u8; 2_000]),
    );
    let state = Rc::new(
        DocState::try_new(Limits::LARGE, 1_000, hosts.in_use().clone())
            .unwrap()
            .with_storage(Some(Rc::new(Card(card.clone(), renames.clone())))),
    );
    hosts.wire_documents(state.clone()).unwrap();
    let world = World {
        env: &env,
        settings: Cell::new(std::boxed::Box::leak(std::boxed::Box::new(Settings::new()))),
        store: RefCell::new(DeclarationStore::default()),
        plugins: RefCell::new(Vec::new()),
        generation: Cell::new(1),
        walks: Cell::new(0),
        sniffs: RefCell::new(Vec::new()),
        sniffed: RefCell::new(Vec::new()),
        card,
        renames,
        makes_cache: Cell::new(false),
        version: Cell::new("1.0.0"),
        scoped: RefCell::new(Vec::new()),
        fail: Cell::new(0),
        reply: RefCell::new(Ok(state_reply())),
        calls: RefCell::new(Vec::new()),
        hosts,
        state,
    };
    body(&world)
}

#[test]
fn formats_and_reading_apps_are_listed_and_selected_from_the_declarations() {
    world(|w| {
        w.install(
            "epub",
            EPUB,
            &[
                format("epub", "epub", 0, 5),
                format("txt", "txt", 0, 0),
                Record::ScreenSpec(wire::ScreenSpec {
                    name: "main",
                    title: "t",
                    token: 1,
                    flags: 0,
                }),
                Record::ReaderSpec(wire::ReaderSpec {
                    name: "app",
                    screen: "epub.main",
                    formats: "",
                    flags: 0,
                }),
            ],
        );
        // The title is a catalog key: shown as the key where none translates it.
        assert_eq!(
            w.list(19).unwrap(),
            [
                ("epub.epub".into(), "format.title".into()),
                ("epub.txt".into(), "format.title".into())
            ]
        );
        // The reading app is shown under its plugin's name.
        assert_eq!(
            w.list(21).unwrap(),
            [("epub.app".into(), "Name of epub".into())]
        );
        assert_eq!(w.format_for("books/a.EPUB").unwrap(), "epub.epub");
        assert_eq!(w.format_for("books/a.zip"), Err(ERR_NOT_FOUND));
        assert_eq!(w.format_for("README"), Err(ERR_NOT_FOUND));
    });
}

#[test]
fn the_users_choice_beats_priority_and_priority_beats_load_order() {
    world(|w| {
        w.install("first", 21, &[format("a", "txt", 0, 0)]);
        w.install("second", 22, &[format("b", "txt", 0, 9)]);
        w.install("third", 23, &[format("c", "txt", 0, 9)]);
        // Priority 9 twice: the first loaded of those.
        assert_eq!(w.format_for("n.txt").unwrap(), "second.b");
        // The user's `core.formats.txt` names a claimant: it wins.
        w.choose("first.a");
        assert_eq!(w.format_for("n.txt").unwrap(), "first.a");
        // A choice that names no claimant of the extension is ignored.
        w.choose("nobody.x");
        assert_eq!(w.format_for("n.txt").unwrap(), "second.b");
    });
}

#[test]
fn a_book_is_opened_through_its_format_and_closed_telling_the_format() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        assert_eq!(w.calls.borrow().as_slice(), [(EPUB, 5, 2)]);
        // Root 1 needs `uses.storage`: not for an instance that lacks it.
        let no = w.call(
            AdmittedFeatures::NONE,
            1,
            &[Record::ServiceDocOpen(wire::ServiceDocOpen {
                root: 1,
                path: "books/a.epub",
                format: "",
                flags: 0,
                image_side: 0,
                cover_side: 0,
                css_root: 0,
                css_path: "",
            })],
        );
        assert!(no.is_err());
        assert_eq!(w.state.table.len(), 1);
        w.call(
            AdmittedFeatures::NONE,
            18,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        )
        .unwrap();
        // Op 4 `close` went to the format; the table is empty.
        assert_eq!(w.calls.borrow().last(), Some(&(EPUB, 5, 4)));
        assert_eq!(w.state.table.len(), 0);
    });
}

#[test]
fn a_book_of_the_card_is_held_until_its_document_closes() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(1, "books/a.epub").unwrap();
        assert!(w.hosts.in_use().holds("books/a.epub", false));
        w.call(
            AdmittedFeatures::NONE,
            18,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        )
        .unwrap();
        assert!(!w.hosts.in_use().holds("books/a.epub", false));
        // A format that fails the open gives the book back too.
        *w.reply.borrow_mut() = Err(ERR_IO);
        assert!(w.open(1, "books/b.epub").is_err());
        assert!(!w.hosts.in_use().holds("books/b.epub", false));
        assert_eq!(w.state.books.borrow().len(), 0);
    });
}

#[test]
fn a_book_of_the_openers_data_is_held_under_its_card_path_until_the_document_closes() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let held = ".paperpaw/plugins/opener/books/a.epub";
        let document = w.open(0, "books/a.epub").unwrap();
        assert!(w.hosts.in_use().holds(held, false));
        // The card's own path of the same name is another file.
        assert!(!w.hosts.in_use().holds("books/a.epub", false));
        w.call(
            AdmittedFeatures::NONE,
            18,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        )
        .unwrap();
        assert!(!w.hosts.in_use().holds(held, false));
        assert_eq!(w.state.books.borrow().len(), 0);
        // A format that fails the open gives it back too.
        *w.reply.borrow_mut() = Err(ERR_IO);
        assert!(w.open(0, "books/b.epub").is_err());
        assert!(
            !w.hosts
                .in_use()
                .holds(".paperpaw/plugins/opener/books/b.epub", false)
        );
        assert_eq!(w.state.books.borrow().len(), 0);
    });
}

#[test]
fn the_longest_root_0_path_is_held_and_a_longer_one_is_refused_before_any_hold() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        // 512 bytes is the longest path `doc_open` takes; the card path that
        // joins it still fits the in-use table's names.
        let longest = "a".repeat(507) + ".epub";
        assert!(w.open(0, &longest).is_ok());
        assert_eq!(w.state.books.borrow().len(), 1);
        let joined = ".paperpaw/plugins/opener/".to_string() + &longest;
        assert!(w.hosts.in_use().holds(&joined, false));
        // One byte more never reaches the host.
        let longer = "a".repeat(508) + ".epub";
        assert!(w.open(0, &longer).is_err());
        assert_eq!(w.state.books.borrow().len(), 1);
    });
}

#[test]
fn a_refused_allocation_holds_no_root_0_book_and_makes_no_document() {
    // The eleventh allocation is the test format's own call log, which
    // cannot be refused; the ten before it are the host's.
    for nth in 1..=10 {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            w.fail.set(nth);
            let opened = w.open(0, "books/a.epub");
            w.fail.set(0);
            if opened.is_err() {
                assert!(
                    !w.hosts
                        .in_use()
                        .holds(".paperpaw/plugins/opener/books/a.epub", false),
                    "allocation {nth}"
                );
                assert_eq!(w.state.books.borrow().len(), 0, "allocation {nth}");
                assert_eq!(w.state.table.len(), 0, "allocation {nth}");
            }
        });
    }
}

#[test]
fn a_format_that_stops_ends_its_documents_and_is_not_called_again() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        let calls = w.calls.borrow().len();
        w.stop("epub");
        // The next call the service makes brings the index in line: the
        // document is the host's to end, the format is told nothing.
        assert_eq!(w.format_for("a.epub"), Err(ERR_NOT_FOUND));
        let state = w.call(
            AdmittedFeatures::NONE,
            2,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        );
        assert_eq!(state.err(), Some(ERR_CANCELLED));
        assert_eq!(w.open(0, "b.epub"), Err(ERR_NOT_FOUND));
        assert_eq!(w.calls.borrow().len(), calls, "never called again");
    });
}

#[test]
fn a_busy_table_leaves_the_documents_of_a_stopped_format_to_the_next_call() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        w.stop("epub");
        {
            let _running = w.state.table.hold().unwrap();
            // The index forgets the format; the table cannot end its book.
            let _ = w.format_for("a.epub");
            assert_eq!(w.state.unfinished(), 1);
        }
        // The next call, with nothing changed in the registry, finishes it.
        assert_eq!(w.format_for("a.epub"), Err(ERR_NOT_FOUND));
        assert_eq!(w.state.unfinished(), 0);
        let state = w.call(
            AdmittedFeatures::NONE,
            2,
            &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
        );
        assert_eq!(state.err(), Some(ERR_CANCELLED));
    });
}

#[test]
fn a_held_book_blocks_a_move_under_another_case_where_the_card_folds_case() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.open(1, "Books/A.epub").unwrap();
        // The files service asks with `ignore_case` for a FAT card.
        assert!(w.hosts.in_use().holds("books/a.epub", true));
        assert!(!w.hosts.in_use().holds("books/a.epub", false));
        assert!(w.hosts.in_use().holds("Books/A.epub", false));
    });
}

#[test]
fn the_index_follows_the_declarations_once_per_change_not_once_per_call() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        for _ in 0..5 {
            w.format_for("a.epub").unwrap();
            w.list(19).unwrap();
        }
        assert_eq!(w.walks.get(), 1);
        w.install("txt", 12, &[format("txt", "txt", 0, 0)]);
        assert_eq!(w.format_for("a.txt").unwrap(), "txt.txt");
        assert_eq!(w.walks.get(), 2);
        assert_eq!(w.state.skipped(), 0);
    });
}

#[test]
fn malformed_input_and_unwired_roots_are_refused_before_a_format_is_called() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let schema = services_wire::service_schema("core.document", 1).unwrap();
        for id in [1_u32, 2, 3, 18, 19, 20, 21] {
            let op = schema.op(id).unwrap();
            assert!(
                w.call_raw(AdmittedFeatures::NONE, op, &[]).is_err(),
                "op {id}"
            );
            assert!(
                w.call_raw(AdmittedFeatures::NONE, op, &[1, 0xff, 0xff])
                    .is_err(),
                "op {id}"
            );
        }
        // `doc_open` names root 0 or 1 only; any other root is invalid.
        assert_eq!(w.open(5, "a.epub"), Err(ERR_INVALID));
        assert!(w.open(9, "a.epub").is_err());
        assert!(w.calls.borrow().is_empty());
        assert_eq!(w.state.table.len(), 0);
    });
}

#[test]
fn a_refused_allocation_leaves_the_index_to_be_made_whole_by_the_next_call() {
    for nth in 1..=24 {
        world(|w| {
            w.install(
                "epub",
                EPUB,
                &[format("epub", "epub", 0, 0), format("txt", "txt", 0, 0)],
            );
            w.install("fb2", 12, &[format("fb2", "fb2", 0, 0)]);
            w.fail.set(nth);
            let first = w.list(19);
            w.fail.set(0);
            if let Ok(rows) = first {
                assert_eq!(rows.len(), 3, "allocation {nth}");
            } else {
                assert_eq!(first.err(), Some(ERR_NO_MEMORY), "allocation {nth}");
            }
            // Without a fault the next call sees everything declared.
            let rows = w.list(19).unwrap();
            assert_eq!(rows.len(), 3, "allocation {nth}");
            assert_eq!(w.state.skipped(), 0);
        });
    }
}

/// The next `event.document_closed` of `owner` as (document, reason).
fn closed_event(w: &World<'_>, owner: u64) -> Result<Option<(u32, u32)>, i32> {
    let mut out = [0_u8; 64];
    let Some(length) = w.state.next_event(owner, &mut out)? else {
        return Ok(None);
    };
    match wire::record_decode_one(&out[..length]).unwrap() {
        Record::EventDocumentClosed(event) => Ok(Some((event.document, event.reason))),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_stopped_format_closes_its_book_and_the_opener_hears_it_once() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        assert_eq!(closed_event(w, OPENER), Ok(None), "nothing happened yet");
        w.stop("epub");
        // The next call brings the index in line: reason 1, the format stopped.
        assert_eq!(w.format_for("a.epub"), Err(ERR_NOT_FOUND));
        assert_eq!(w.format_for("a.epub"), Err(ERR_NOT_FOUND));
        w.state.set_now(1_234);
        assert_eq!(closed_event(w, OPENER), Ok(Some((document, 1))));
        assert_eq!(closed_event(w, OPENER), Ok(None), "one event per close");
    });
}

#[test]
fn only_the_opener_of_a_book_is_told_and_one_that_closes_it_hears_nothing() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        let other = w.open(0, "b.epub").unwrap();
        assert_ne!(document, other);
        w.call(
            AdmittedFeatures::NONE,
            18,
            &[Record::ServiceDocRef(wire::ServiceDocRef {
                document: other,
            })],
        )
        .unwrap();
        doc_service::end_by_host(&*w.state, document, 3).unwrap();
        // A document that ended is left alone by a second ending.
        doc_service::end_by_host(&*w.state, document, 2).unwrap();
        assert_eq!(closed_event(w, OPENER + 1), Ok(None), "not their book");
        assert!(w.state.table.has_closed());
        assert_eq!(closed_event(w, OPENER), Ok(Some((document, 3))));
        assert_eq!(closed_event(w, OPENER), Ok(None));
        assert!(!w.state.table.has_closed());
    });
}

#[test]
fn an_opener_that_stopped_leaves_no_event_behind() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.open(0, "a.epub").unwrap();
        w.stop("epub");
        let _ = w.format_for("a.epub");
        assert!(w.state.table.has_closed());
        doc_service::opener_stopped(&*w.state, OPENER).unwrap();
        assert_eq!(closed_event(w, OPENER), Ok(None));
        assert!(!w.state.table.has_closed());
    });
}

#[test]
fn an_event_that_does_not_fit_stays_until_there_is_room() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        doc_service::end_by_host(&*w.state, document, 2).unwrap();
        let mut small = [0_u8; 4];
        assert!(w.state.next_event(OPENER, &mut small).is_err());
        assert_eq!(closed_event(w, OPENER), Ok(Some((document, 2))));
    });
}

#[test]
fn a_refused_allocation_makes_no_document_state_and_no_event_allocates() {
    let in_use = HostServices::new().in_use().clone();
    for nth in 1..=8 {
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(nth));
        let made = DocState::try_new(Limits::LARGE, 1_000, in_use.clone());
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(0));
        if let Err(status) = made {
            assert_eq!(status, ERR_NO_MEMORY, "allocation {nth}");
        }
    }
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        doc_service::end_by_host(&*w.state, document, 2).unwrap();
        // Building the event takes no allocation: refusing every one is no
        // matter.
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(1));
        let event = closed_event(w, OPENER);
        let armed = crate::native_host::shell::tests::FAIL_NTH.with(Cell::get);
        crate::native_host::shell::tests::FAIL_NTH.with(|n| n.set(0));
        assert_eq!(event, Ok(Some((document, 2))));
        assert_eq!(armed, 1, "no allocation was asked for");
    });
}

#[test]
fn an_event_asks_for_one_pass_and_one_for_an_owner_that_does_not_run_stays_quiet() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(0, "a.epub").unwrap();
        assert!(!w.state.events_waiting());
        doc_service::end_by_host(&*w.state, document, 2).unwrap();
        assert!(w.state.events_waiting(), "the pump is asked for a pass");
        // The pass starts, but the owner is not running: nothing takes the
        // event, and nothing asks for another pass for it.
        w.state.set_now(10);
        assert!(!w.state.events_waiting());
        assert!(w.state.table.has_closed());
        // The owner's callback runs on a later pass and still hears it.
        assert_eq!(closed_event(w, OPENER), Ok(Some((document, 2))));
    });
}

/// Two formats that sniff: `fb2` (owner 12, priority 5) asked first, `epub`
/// (owner 11).
fn sniffers(w: &World<'_>) {
    const SNIFFS: u32 = 1 << 1;
    w.install("epub", EPUB, &[format("epub", "epub", SNIFFS, 0)]);
    w.install("fb2", 12, &[format("fb2", "fb2", SNIFFS, 5)]);
}

fn ops(w: &World<'_>) -> Vec<(u64, u32)> {
    w.calls.borrow().iter().map(|c| (c.0, c.2)).collect()
}

#[test]
fn a_file_no_format_claims_is_read_by_the_first_sniffer_to_say_yes() {
    world(|w| {
        sniffers(w);
        w.sniffs.borrow_mut().push(EPUB);
        // No extension, then an extension no format has.
        assert!(w.open(1, "mystery").is_ok());
        assert_eq!(ops(w), [(12, 1), (EPUB, 1), (EPUB, 2)]);
        w.calls.borrow_mut().clear();
        assert!(w.open(1, "odd.zzz").is_ok());
        assert_eq!(ops(w), [(12, 1), (EPUB, 1), (EPUB, 2)]);
    });
}

#[test]
fn nothing_is_asked_of_a_sniffer_for_a_file_a_format_claims() {
    world(|w| {
        sniffers(w);
        w.open(1, "a.fb2").unwrap();
        assert_eq!(ops(w), [(12, 2)]);
    });
}

#[test]
fn a_file_no_sniffer_knows_is_not_found_and_a_missing_file_asks_none() {
    world(|w| {
        sniffers(w);
        assert_eq!(w.open(1, "mystery"), Err(ERR_NOT_FOUND));
        assert_eq!(ops(w), [(12, 1), (EPUB, 1)]);
        w.calls.borrow_mut().clear();
        // No such file: the card says so, no format is asked.
        assert_eq!(w.open(1, "absent"), Err(ERR_NOT_FOUND));
        assert!(w.calls.borrow().is_empty());
        // Without the sniffs formats nothing reads a stranger.
        let none = world_without_sniffers();
        assert_eq!(none, Err(ERR_NOT_FOUND));
    });
}

fn world_without_sniffers() -> Result<u32, i32> {
    let mut result = Ok(0);
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        result = w.open(1, "mystery");
        assert!(w.calls.borrow().is_empty());
    });
    result
}

#[test]
fn op_20_sniffs_only_when_asked_and_a_sniffer_never_gets_more_than_512_bytes() {
    world(|w| {
        sniffers(w);
        w.sniffs.borrow_mut().push(12);
        let ask = |flags: u32| {
            w.call(
                AdmittedFeatures::test(1, FEATURES),
                20,
                &[Record::ServiceDocPath(wire::ServiceDocPath {
                    root: 1,
                    path: "big",
                    flags,
                })],
            )
        };
        assert_eq!(ask(0).err(), Some(ERR_NOT_FOUND));
        assert!(w.calls.borrow().is_empty(), "no sniff unless asked");
        let reply = ask(1).unwrap();
        match wire::record_decode_one(&reply).unwrap() {
            Record::ServiceDocFormat(format) => assert_eq!(format.id, "fb2.fb2"),
            other => panic!("{other:?}"),
        }
        assert_eq!(*w.sniffed.borrow(), [512]);
    });
}

#[test]
fn a_refused_allocation_while_sniffing_is_no_memory_and_nothing_else() {
    for nth in 1..=12 {
        world(|w| {
            sniffers(w);
            w.sniffs.borrow_mut().push(EPUB);
            w.fail.set(nth);
            let result = w.open(1, "mystery");
            w.fail.set(0);
            assert!(
                matches!(result, Ok(_) | Err(ERR_NO_MEMORY)),
                "allocation {nth}: {result:?}"
            );
            // Nothing is left half-made: the next call reads the file.
            assert!(w.open(1, "mystery").is_ok(), "allocation {nth}");
        });
    }
}

#[test]
fn a_format_call_is_for_its_book_and_the_scope_ends_with_it() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let document = w.open(1, "books/a.epub").unwrap();
        assert_eq!(
            *w.scoped.borrow(),
            [(
                "books/a.epub".into(),
                BookKind::File,
                "".into(),
                document,
                "".into(),
                seen_cache(w, 0)
            )]
        );
        // Nobody is in a call any more.
        assert!(!w.hosts.format_scope().current(EPUB, &mut |_| {}));
        // The root-0 book is named by its card path.
        w.scoped.borrow_mut().clear();
        w.open(0, "b.epub").unwrap();
        assert_eq!(w.scoped.borrow()[0].0, ".paperpaw/plugins/opener/b.epub");
    });
}

#[test]
fn a_siblings_or_folder_format_gets_its_kind_and_the_opened_name() {
    const FOLDER: u32 = 1 << 2;
    const SIBLINGS: u32 = 1 << 4;
    world(|w| {
        w.install("cbz", 11, &[format("cbz", "cbz", SIBLINGS, 0)]);
        w.install("dir", 12, &[format("dir", "dir", FOLDER, 0)]);
        w.open(1, "comics/a.cbz").unwrap();
        w.open(1, "comics/b.dir").unwrap();
        let seen = w.scoped.borrow();
        assert_eq!(seen[0].1, BookKind::Siblings);
        assert_eq!(seen[0].2, "a.cbz");
        assert_eq!(seen[1].1, BookKind::Folder);
        assert_eq!(seen[1].2, "");
    });
}

#[test]
fn the_css_file_is_root_4_in_the_open_only_by_its_card_path() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let close = |document: u32| {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        };
        close(
            w.open_with_css(1, "books/a.epub", 1, "styles/u.css")
                .unwrap(),
        );
        close(w.open_with_css(1, "books/b.epub", 0, "u.css").unwrap());
        close(w.open(1, "books/c.epub").unwrap());
        let seen = w.scoped.borrow();
        assert_eq!(seen[0].4, "styles/u.css");
        assert_eq!(seen[1].4, ".paperpaw/plugins/opener/u.css");
        assert_eq!(seen[2].4, "");
    });
}

#[test]
fn a_call_that_cannot_enter_its_scope_is_refused_and_a_refused_allocation_leaves_none() {
    for nth in 1..=40 {
        world(|w| {
            w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
            w.fail.set(nth);
            let opened = w.open(1, "books/a.epub");
            w.fail.set(0);
            assert!(
                matches!(opened, Ok(_) | Err(ERR_NO_MEMORY | ERR_FULL)),
                "allocation {nth}: {opened:?}"
            );
            assert!(!w.hosts.format_scope().current(EPUB, &mut |_| {}));
        });
    }
}

/// The cache folder the format's call number `at` saw.
fn seen_cache(w: &World<'_>, at: usize) -> std::string::String {
    w.scoped.borrow()[at].5.clone()
}

fn conversion_of(folder: &str) -> &str {
    folder.rsplit('/').next().unwrap()
}

#[test]
fn an_open_has_its_cache_folder_and_a_finished_open_is_stamped_for_its_conversion() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        let close = |document: u32| {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        };
        close(w.open(1, "books/a.epub").unwrap());
        let folder = seen_cache(w, 0);
        assert!(folder.starts_with(".paperpaw/cache/"), "{folder}");
        assert!(folder.contains("/epub/"), "{folder}");
        assert_eq!(conversion_of(&folder).len(), 16);
        // The format answered ready: the stamp is made.
        assert!(
            w.card
                .get(&std::format!(
                    "{folder}/{}",
                    crate::native_host::doc_cache::STAMP
                ))
                .is_some()
        );
        // The finished cache is kept for the next open of the same conversion.
        close(w.open(1, "books/a.epub").unwrap());
        assert_eq!(seen_cache(w, 1), folder);
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_some());
        // A new version of the format makes it stale: it is made again.
        w.version.set("2.0.0");
        w.makes_cache.set(false);
        close(w.open(1, "books/a.epub").unwrap());
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_none());
        // Another book has another cache.
        close(w.open(1, "books/b.epub").unwrap());
        assert_ne!(seen_cache(w, 3), folder);
    });
}

#[test]
fn a_half_made_cache_is_removed_at_the_next_open_and_a_ready_open_that_made_none_is_not_stamped() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        let close = |document: u32| {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        };
        close(w.open(1, "books/a.epub").unwrap());
        let folder = seen_cache(w, 0);
        assert!(
            w.card
                .get(&std::format!(
                    "{folder}/{}",
                    crate::native_host::doc_cache::STAMP
                ))
                .is_none()
        );
        // A cache of a cut preparation: pages and no stamp.
        w.card
            .write(&std::format!("{folder}/pages/9"), b"old")
            .unwrap();
        close(w.open(1, "books/a.epub").unwrap());
        assert!(w.card.get(&std::format!("{folder}/pages/9")).is_none());
    });
}

/// Opens `path` (with the CSS file `css` of root 1 when it is not empty) and
/// closes the document again; the folder of its cache is the format's call
/// number `at`.
fn open_and_close(w: &World<'_>, path: &str, css: &str) {
    let document = if css.is_empty() {
        w.open(1, path)
    } else {
        w.open_with_css(1, path, 1, css)
    }
    .unwrap();
    w.call(
        AdmittedFeatures::NONE,
        18,
        &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
    )
    .unwrap();
}

#[test]
fn a_book_replaced_at_its_path_or_an_edited_css_file_makes_the_cache_again() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"first edition").unwrap();
        w.card.write("style.css", b"p { color: red }").unwrap();
        open_and_close(w, "books/a.epub", "style.css");
        let folder = seen_cache(w, 0);
        let page = std::format!("{folder}/pages/1");
        w.card.write(&page, b"old text").unwrap();
        // The same book and CSS file: the finished cache is used as it is.
        w.makes_cache.set(false);
        open_and_close(w, "books/a.epub", "style.css");
        assert_eq!(w.card.get(&page).as_deref(), Some(&b"old text"[..]));
        // Another edition of the book at the same path, even of the same size.
        w.card.write("books/a.epub", b"FIRST EDITION").unwrap();
        open_and_close(w, "books/a.epub", "style.css");
        assert!(w.card.get(&page).is_none(), "the old book's cache is used");
        // The cache of the new edition is finished in turn; then the CSS file
        // is edited (other bytes, same size).
        w.makes_cache.set(true);
        open_and_close(w, "books/a.epub", "style.css");
        w.card.write(&page, b"new text").unwrap();
        w.makes_cache.set(false);
        open_and_close(w, "books/a.epub", "style.css");
        assert_eq!(w.card.get(&page).as_deref(), Some(&b"new text"[..]));
        w.card.write("style.css", b"p { color: RED }").unwrap();
        open_and_close(w, "books/a.epub", "style.css");
        assert!(w.card.get(&page).is_none(), "the old CSS's cache is used");
    });
}

#[test]
fn a_finished_cache_found_at_the_open_keeps_its_stamp() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"book").unwrap();
        open_and_close(w, "books/a.epub", "");
        let folder = seen_cache(w, 0);
        let stamp = std::format!("{folder}/{}", crate::native_host::doc_cache::STAMP);
        let first = w.card.get(&stamp).unwrap();
        // One rename made it (from the host's folder); finding it current at
        // the next opens makes no other: the host does not make it again.
        assert_eq!(w.renames.get(), 1);
        open_and_close(w, "books/a.epub", "");
        open_and_close(w, "books/a.epub", "");
        assert_eq!(w.renames.get(), 1);
        assert_eq!(w.card.get(&stamp).unwrap(), first);
    });
}

#[test]
fn a_cache_that_ran_out_of_room_is_removed_with_what_was_counted() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"book").unwrap();
        *w.reply.borrow_mut() = Err(ERR_NO_SPACE);
        assert_eq!(w.open(1, "books/a.epub"), Err(ERR_NO_SPACE));
        let folder = seen_cache(w, 0);
        assert!(!folder.is_empty());
        assert!(w.card.get(&std::format!("{folder}/pages/1")).is_none());
        assert_eq!(w.hosts.format_scope().total(&folder), 0);
    });
}

#[test]
fn what_a_format_wrote_after_the_stamp_is_in_the_stamp_when_its_last_document_closes() {
    world(|w| {
        w.install("epub", EPUB, &[format("epub", "epub", 0, 0)]);
        w.makes_cache.set(true);
        w.card.write("books/a.epub", b"book").unwrap();
        let id = w.open(1, "books/a.epub").unwrap();
        let folder = seen_cache(w, 0);
        let stamp = std::format!("{folder}/{}", crate::native_host::doc_cache::STAMP);
        let bytes = |w: &World<'_>| {
            let text = w.card.get(&stamp).unwrap();
            let line = text.rsplit(|b| *b == b'\n').nth(1).unwrap().to_vec();
            std::string::String::from_utf8(line).unwrap()
        };
        // The format answered ready with nothing counted; it writes on later.
        assert_eq!(bytes(w), "0");
        let scope = w.hosts.format_scope();
        scope.charge(&folder, 700, 0, u64::MAX).unwrap();
        // A second document of the same cache keeps the count.
        let second = w.open(1, "books/a.epub").unwrap();
        let close = |document: u32| {
            w.call(
                AdmittedFeatures::NONE,
                18,
                &[Record::ServiceDocRef(wire::ServiceDocRef { document })],
            )
            .unwrap();
        };
        close(id);
        assert_eq!((bytes(w), scope.total(&folder)), ("0".into(), 700));
        close(second);
        // The last one closed: the stamp says it, the count starts again
        // from the stamp at the next open.
        assert_eq!((bytes(w), scope.total(&folder)), ("700".into(), 0));
        open_and_close(w, "books/a.epub", "");
        assert_eq!(bytes(w), "700");
    });
}
