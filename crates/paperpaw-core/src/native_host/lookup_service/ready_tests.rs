#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! A pending lookup ends through the kernel (`document.md` 7): the provider's
//! callback stages `command.lookup_ready`, the kernel applies it after the
//! callback, the event pump hands `event.lookup_ready` to the asker that
//! subscribed, and the asker's next ask has the entry.
extern crate std;
use super::*;
use crate::abi7::services_wire;
use crate::native_host::{
    commands::{CommandStage, WireCommand, commands_decode, commands_encode},
    event_tags::EventTags,
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
};
use alloc::{string::String, vec::Vec};
use core::cell::{Cell, RefCell};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NOT_FOUND, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, Settings};
use wire::{ServiceLookupAsk as Ask, ServiceLookupEntry as Entry};

/// The provider "wiki": pending until its result is kept.
struct Host {
    wiki: Cell<u64>,
    kept: Cell<bool>,
}
impl LookupHost for Host {
    fn ask(&self, _: &dyn Frame, _: u64, ask: &Ask<'_>, out: &mut [u8]) -> Result<usize, i32> {
        if ask.cursor > 0 {
            return Err(ERR_NOT_FOUND);
        }
        let kept = self.kept.get();
        wire::record_encode_one(
            &Record::ServiceLookupEntry(Entry {
                provider: "wiki",
                title: "Tree",
                text: if kept { "A woody plant." } else { "" },
                flags: if kept { 0 } else { PENDING },
                opens: "",
            }),
            out,
        )
    }
    fn provider_owner(&self, _: &dyn Frame, name: &str) -> Option<u64> {
        (name == "wiki").then(|| self.wiki.get())
    }
    // Not the store's (the kernel of the test has none): the same name.
    fn provider_name(&self, _: Option<&DeclarationStore>, owner: u64) -> Option<([u8; 64], u8)> {
        let mut name = [0; 64];
        name[..4].copy_from_slice(b"wiki");
        (owner == self.wiki.get()).then_some((name, 4))
    }
}

/// A plugin: the asker records the events it gets; the provider stages
/// `command.lookup_ready` for "tree" in its callback.
struct Plugin {
    provider: bool,
    events: Rc<RefCell<Vec<Vec<u8>>>>,
    staged: Rc<Cell<i32>>,
}
impl NativeInstance for Plugin {
    fn running(&self) -> bool {
        true
    }
    fn stop(&self) {}
    fn subscribed(&self, tag: u32) -> bool {
        !self.provider && tag == wire::EVENT_LOOKUP_READY_TAG
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        _: &paperpaw_sdk::Env<'_>,
        stage: &mut CommandStage<'_>,
    ) -> Result<i32, i32> {
        if !self.provider {
            self.events.borrow_mut().push(bytes.into());
            return Ok(OK);
        }
        let mut out = [0; 64];
        let ready = WireCommand::LookupReady {
            text: "tree",
            language: "en",
        };
        let len = commands_encode(&[ready], &mut out).unwrap();
        self.staged
            .set(stage.stage(&commands_decode(&out[..len]).unwrap()));
        Ok(OK)
    }
}
impl Dispatch for Plugin {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    asker: u64,
    provider: (u64, NativeLease),
    host: Rc<Host>,
    events: Rc<RefCell<Vec<Vec<u8>>>>,
    staged: Rc<Cell<i32>>,
}
impl Frame for World {
    fn settings(&self) -> &Settings {
        self.kernel.shared.env(Instant(0)).settings()
    }
    fn translator(&self) -> &Translator {
        unreachable!("the lookup backend never translates")
    }
}

fn world() -> World {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let events = Rc::new(RefCell::new(Vec::new()));
    let staged = Rc::new(Cell::new(OK));
    let plugin = |provider| Plugin {
        provider,
        events: events.clone(),
        staged: staged.clone(),
    };
    let mut insert = |id: &'static str, provider: bool| {
        let lease = NativeLease::try_new(plugin(provider)).unwrap();
        kernel
            .shared
            .native_instances
            .test_insert(Id::new_static(id), Some(lease), true)
            .unwrap()
    };
    let asker = insert("reader", false);
    let wiki = insert("wiki", true);
    let host = Rc::new(Host {
        wiki: Cell::new(wiki),
        kept: Cell::new(false),
    });
    let hosts = kernel.shared.native_instances.hosts().unwrap();
    hosts.wire_lookup(host.clone(), Default::default()).unwrap();
    // The asker subscribed to the event, as its install records it.
    let owners = hosts.owners().unwrap();
    owners.admit(asker, &Id::new_static("reader")).unwrap();
    let mut tags = EventTags::default();
    tags.insert(wire::EVENT_LOOKUP_READY_TAG).unwrap();
    owners.subscribe(asker, tags);
    World {
        kernel,
        asker,
        provider: (wiki, NativeLease::try_new(plugin(true)).unwrap()),
        host,
        events,
        staged,
    }
}

impl World {
    /// The asker's `lookup` of "tree": provider, flags, text.
    fn ask(&self) -> (String, u32, String) {
        let op = services_wire::service_schema("core.lookup", 1)
            .unwrap()
            .op(1)
            .unwrap();
        let mut input = std::vec![0; 4096];
        let ask = Ask {
            text: "tree",
            language: "en",
            context: "",
            cursor: 0,
        };
        let used = wire::records_encode_counted(
            &[Record::ServiceLookupAsk(ask)],
            1,
            op.max_input,
            &mut input,
        )
        .unwrap();
        let caller = Caller {
            owner: self.asker,
            plugin: "reader",
            features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
            hosts: self.kernel.shared.native_instances.hosts().unwrap(),
        };
        let mut out = std::vec![0; op.max_reply];
        let length = backend(self, &caller, op, &input[..used], &mut out).unwrap();
        let Record::ServiceLookupEntry(entry) = wire::record_decode_one(&out[..length]).unwrap()
        else {
            panic!("an entry")
        };
        (entry.provider.into(), entry.flags, entry.text.into())
    }
    fn service(&self) -> &LookupServices {
        self.kernel
            .shared
            .native_instances
            .hosts()
            .unwrap()
            .lookup()
            .unwrap()
    }
}

#[test]
fn a_ready_command_reaches_the_asker_as_event_110_and_the_next_ask_has_the_entry() {
    let mut w = world();
    // The provider must fetch: pending, and the ask is kept for the asker.
    assert_eq!(w.ask(), ("wiki".into(), PENDING, String::new()));
    assert_eq!(w.service().pending_len(), 1);
    // Its fetch ends: its callback keeps the result and says so.
    w.host.kept.set(true);
    let (wiki, lease) = &w.provider;
    assert_eq!(w.kernel.native_event_call(*wiki, lease, &[]), Ok(OK));
    assert_eq!(w.staged.get(), OK, "command.lookup_ready is staged");
    // Applied after the callback: the ask ends, one event waits.
    w.kernel.native_outbox_run();
    assert_eq!(w.service().pending_len(), 0);
    assert_eq!(w.service().events_len(), 1);
    // The pump hands it to the asker, once.
    w.kernel.now = Instant(5_000);
    w.kernel.native_service_pump();
    w.kernel.native_service_pump();
    let events = w.events.borrow().clone();
    assert_eq!(events.len(), 1, "one event 110");
    let Record::EventLookupReady(event) = wire::records_decode_single(&events[0]).unwrap() else {
        panic!("event.lookup_ready")
    };
    assert_eq!(event.provider, "wiki");
    assert_eq!(w.service().events_len(), 0);
    // The asker asks again and has the entry, no longer pending.
    assert_eq!(w.ask(), ("wiki".into(), 0, "A woody plant.".into()));
}

#[test]
fn a_ready_command_without_a_pending_ask_is_accepted_and_tells_no_one() {
    let mut w = world();
    let (wiki, lease) = &w.provider;
    assert_eq!(w.kernel.native_event_call(*wiki, lease, &[]), Ok(OK));
    assert_eq!(w.staged.get(), OK, "never an error to the provider");
    w.kernel.native_outbox_run();
    assert_eq!(w.service().unmatched(), 1);
    assert_eq!(w.service().events_len(), 0);
}

#[test]
fn an_asker_that_stops_loses_its_waiting_event() {
    let mut w = world();
    w.ask();
    let (wiki, lease) = &w.provider;
    w.kernel.native_event_call(*wiki, lease, &[]).unwrap();
    w.kernel.native_outbox_run();
    assert_eq!(w.service().events_len(), 1);
    w.service().stopped(w.asker);
    assert_eq!(w.service().events_len(), 0);
}

#[test]
fn a_ready_command_needs_a_text_and_a_language_the_ask_takes() {
    let w = world();
    let owner = Id::new_static("wiki");
    let settings = w.kernel.shared.env(Instant(0));
    let mut stage = CommandStage::new(&owner, settings.settings(), None, 8, 4096);
    for (text, language) in [("", "en"), ("tree", "e"), ("tree", "en_US")] {
        let mut out = [0; 64];
        let len =
            commands_encode(&[WireCommand::LookupReady { text, language }], &mut out).unwrap();
        let batch = commands_decode(&out[..len]).unwrap();
        assert_eq!(stage.stage(&batch), ERR_INVALID, "{text:?} {language:?}");
    }
    assert!(stage.take_lookup_readies().is_empty());
}
