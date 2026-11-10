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
    doc_service,
    host_services::HostServices,
    registrations::{REGISTRATION_MAX_BYTES, records_encode},
    ui_guard::{Row, Rows, UiStackView},
};
use alloc::{string::ToString, vec};
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::ERR_IO;
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
        _: u32,
        _: &str,
        _: &Asker<'_>,
        _: &mut dyn FnMut(&dyn RegRow) -> Result<(), i32>,
    ) -> Result<(), i32> {
        Err(ERR_UNSUPPORTED)
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
        let Record::StageCall(call) = wire::records_decode_single(input).unwrap() else {
            panic!("not a stage.call")
        };
        let op = match local::input_decode(call.payload)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
        {
            local::Record::FmtCall(c) => c.op,
            other => panic!("{other:?}"),
        };
        self.calls.borrow_mut().push((owner, token, op));
        let answer = self.reply.borrow().clone()?;
        reply[..answer.len()].copy_from_slice(&answer);
        accept(&reply[..answer.len()])?;
        Ok(answer.len())
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
                css_root: 0,
                css_path: "",
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
    let state = Rc::new(DocState::try_new(Limits::LARGE, 1_000, hosts.in_use().clone()).unwrap());
    hosts.wire_documents(state.clone()).unwrap();
    let world = World {
        env: &env,
        settings: Cell::new(std::boxed::Box::leak(std::boxed::Box::new(Settings::new()))),
        store: RefCell::new(DeclarationStore::default()),
        plugins: RefCell::new(Vec::new()),
        generation: Cell::new(1),
        walks: Cell::new(0),
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
