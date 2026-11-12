#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! The three families end to end: `service.open` and `service.call` through
//! the device registry into the wired host views, and what happens to an
//! owner's documents and transactions when it is retired.
extern crate std;
use crate::native_host::resources;
use crate::{
    abi7::services_wire::{self, ServiceOp},
    native_host::{
        HostResources,
        admitted_features::AdmittedFeatures,
        call_services,
        declared_services::DeclaredServices,
        doc_service::tests::{Fake, doc_ref, open_record, state_answer, state_of},
        doc_service::{DocHost, Limits},
        environment::EnvironmentScope,
        lib_service::{LibExcluded, LibQuery, LibWrite, Transactions},
        lookup_service::LookupHost,
        registrations::{self, Record},
    },
};
use core::pin::pin;
use paperpaw_native::host_glue::ERR_FULL;
use paperpaw_native::{
    host_glue::{
        ERR_BUSY, ERR_DENIED, ERR_NOT_FOUND, ERR_STATE, ERR_UNSUPPORTED, OK, PpBuffer, PpBytes,
    },
    metadata::{Declared, Feature},
};
use paperpaw_platform::Instant;
use std::{rc::Rc, vec::Vec};

const WIRED: &[Feature<'static>] = &[Feature {
    key: "uses.library",
    revision: 1,
}];

fn declared(keys: &[&'static str], features: AdmittedFeatures) -> DeclaredServices {
    DeclaredServices::try_from_iter(keys.iter().map(|key| Declared {
        key,
        major: 1,
        semantic_revision: 0,
    }))
    .unwrap()
    .with_features(features)
}

fn encode(family: &str, op: u32, records: &[Record<'_>]) -> Vec<u8> {
    let op = services_wire::service_schema(family, 1)
        .unwrap()
        .op(op)
        .unwrap();
    let mut bytes = std::vec![0; 4096];
    let used = registrations::records_encode_counted(
        records,
        usize::from(op.input_rows),
        op.max_input,
        &mut bytes,
    )
    .unwrap();
    bytes.truncate(used);
    bytes
}

/// One `service.call` as a plugin makes it: the reply bytes or the status.
fn call(
    scope: &EnvironmentScope,
    owner: &resources::OwnerResources,
    handle: u32,
    op: &ServiceOp,
    input: &[u8],
) -> Result<Vec<u8>, i32> {
    let mut out = std::vec![0; op.max_reply];
    // SAFETY: the fixture's input and output are live and disjoint.
    let status = unsafe {
        call_services::call(
            scope,
            owner,
            "opener",
            handle,
            op.id,
            PpBytes {
                ptr: input.as_ptr(),
                len: input.len(),
            },
            PpBuffer {
                ptr: out.as_mut_ptr(),
                cap: out.len(),
            },
        )
    };
    if status < 0 {
        return Err(status);
    }
    out.truncate(status as usize);
    Ok(out)
}

fn open(
    scope: &EnvironmentScope,
    owner: &resources::OwnerResources,
    declared: &DeclaredServices,
    key: &str,
) -> Result<u32, i32> {
    let mut handle = 0;
    // SAFETY: the key and the scalar output are live and disjoint.
    let status = unsafe {
        call_services::open(
            scope,
            owner,
            declared,
            PpBytes {
                ptr: key.as_ptr(),
                len: key.len(),
            },
            1,
            0,
            &mut handle,
        )
    };
    if status == OK {
        Ok(handle)
    } else {
        Err(status)
    }
}

fn op(family: &str, id: u32) -> &'static ServiceOp {
    services_wire::service_schema(family, 1)
        .unwrap()
        .op(id)
        .unwrap()
}

macro_rules! in_frame {
    ($owner:expr, |$scope:ident| $body:block) => {{
        let $scope = pin!(EnvironmentScope::with_resources($owner.try_view().unwrap()));
        let kernel = crate::Kernel::builder(crate::tests::DEVICE)
            .build()
            .unwrap();
        let env = kernel.shared.env(Instant(0));
        $scope.as_ref().scope(&env, || $body).unwrap()
    }};
}

#[test]
fn the_device_registry_offers_the_three_families() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    for key in ["core.document", "core.library", "core.lookup"] {
        assert!(owner.service_schema(key).is_ok(), "{key}");
    }
}

#[test]
fn a_family_the_host_has_not_wired_answers_unsupported() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let declared = declared(&["core.document", "core.lookup"], AdmittedFeatures::NONE);
    in_frame!(owner, |scope| {
        let document = open(&scope, &owner, &declared, "core.document").unwrap();
        let open_op = op("core.document", 1);
        let input = encode("core.document", 1, &[Record::ServiceDocOpen(open_record())]);
        assert_eq!(
            call(&scope, &owner, document, open_op, &input),
            Err(ERR_UNSUPPORTED)
        );
    });
}

#[test]
fn a_document_opens_reads_state_and_ends_with_its_opener() {
    let pool = HostResources::try_new().unwrap();
    let fake = Fake::new(Limits::LARGE);
    let host: Rc<dyn DocHost> = fake.clone();
    pool.hosts().wire_documents(host).unwrap();
    assert_eq!(
        pool.hosts().wire_documents(fake.clone()),
        Err(ERR_STATE),
        "wired once"
    );
    let owner = pool.try_owner().unwrap();
    fake.owner
        .set(owner.caller("opener", AdmittedFeatures::NONE).owner);
    let declared = declared(&["core.document"], AdmittedFeatures::NONE);
    let id = in_frame!(owner, |scope| {
        let handle = open(&scope, &owner, &declared, "core.document").unwrap();
        *fake.answer.borrow_mut() = state_answer(0, 0);
        let input = encode("core.document", 1, &[Record::ServiceDocOpen(open_record())]);
        let reply = call(&scope, &owner, handle, op("core.document", 1), &input).unwrap();
        let id = state_of(&reply).document;
        // Op 2 on the same id, through the same handle.
        let input = encode("core.document", 2, &[doc_ref(id)]);
        let reply = call(&scope, &owner, handle, op("core.document", 2), &input).unwrap();
        assert_eq!(state_of(&reply).document, id);
        // A malformed batch is refused before the host sees it.
        assert!(call(&scope, &owner, handle, op("core.document", 2), &[0xff]).is_err());
        id
    });
    assert!(fake.ended.borrow().is_empty());
    // The opener is retired: its document ends and the format is told once.
    owner.release_all().unwrap();
    assert_eq!(*fake.ended.borrow(), [id]);
    owner.release_all().unwrap();
    assert_eq!(*fake.ended.borrow(), [id], "retiring twice calls nothing");
}

#[test]
fn a_drop_that_meets_a_busy_table_is_retried_by_the_main_loop_until_done() {
    let pool = HostResources::try_new().unwrap();
    let fake = Fake::new(Limits::LARGE);
    let host: Rc<dyn DocHost> = fake.clone();
    pool.hosts().wire_documents(host).unwrap();
    let owner = pool.try_owner().unwrap();
    fake.owner
        .set(owner.caller("opener", AdmittedFeatures::NONE).owner);
    let declared = declared(&["core.document"], AdmittedFeatures::NONE);
    let id = in_frame!(owner, |scope| {
        let handle = open(&scope, &owner, &declared, "core.document").unwrap();
        *fake.answer.borrow_mut() = state_answer(0, 0);
        let input = encode("core.document", 1, &[Record::ServiceDocOpen(open_record())]);
        let reply = call(&scope, &owner, handle, op("core.document", 1), &input).unwrap();
        state_of(&reply).document
    });
    {
        // A call holds the document table while the owner drops.
        let _busy = fake.table().hold().unwrap();
        drop(owner);
        assert!(fake.ended.borrow().is_empty(), "nothing could end yet");
        assert_eq!(pool.retry_retirements(), Err(ERR_BUSY), "still busy");
        assert!(pool.hosts().retirements_pending());
    }
    // The table is free: the main loop finishes the retirement, once.
    assert_eq!(pool.retry_retirements(), Ok(false));
    assert_eq!(*fake.ended.borrow(), [id]);
    assert!(!pool.hosts().retirements_pending());
    assert_eq!(pool.retry_retirements(), Ok(false));
    assert_eq!(*fake.ended.borrow(), [id], "the close count is still one");
}

#[test]
fn many_owners_dropped_under_a_busy_table_are_all_kept_and_retired_once() {
    let pool = HostResources::try_new().unwrap();
    let fake = Fake::new(Limits::LARGE);
    let host: Rc<dyn DocHost> = fake.clone();
    pool.hosts().wire_documents(host).unwrap();
    let declared = declared(&["core.document"], AdmittedFeatures::NONE);
    // 40 owners, far more than a small fixed queue: 8 of them hold a document.
    let owners: Vec<_> = (0..40).map(|_| pool.try_owner().unwrap()).collect();
    let mut ids = Vec::new();
    for owner in &owners[..8] {
        fake.owner
            .set(owner.caller("opener", AdmittedFeatures::NONE).owner);
        ids.push(in_frame!(owner, |scope| {
            let handle = open(&scope, owner, &declared, "core.document").unwrap();
            *fake.answer.borrow_mut() = state_answer(0, 0);
            let input = encode("core.document", 1, &[Record::ServiceDocOpen(open_record())]);
            let reply = call(&scope, owner, handle, op("core.document", 1), &input).unwrap();
            state_of(&reply).document
        }));
    }
    {
        let _busy = fake.table().hold().unwrap();
        drop(owners);
        assert!(fake.ended.borrow().is_empty());
        assert_eq!(pool.retry_retirements(), Err(ERR_BUSY));
        assert!(pool.hosts().retirements_pending());
    }
    assert_eq!(pool.retry_retirements(), Ok(false));
    let mut ended = fake.ended.borrow().clone();
    ended.sort_unstable();
    ids.sort_unstable();
    assert_eq!(ended, ids, "every document ended, once");
    assert!(!pool.hosts().retirements_pending());
    // New owners work after the queue emptied, and drop quietly.
    let again = pool.try_owner().unwrap();
    drop(again);
    assert_eq!(pool.retry_retirements(), Ok(false));
}

#[test]
fn a_retirement_that_cannot_be_queued_is_reported_to_every_retry() {
    let hosts = crate::native_host::host_services::HostServices::new();
    // An owner that was never admitted has no room reserved: the broken
    // invariant is not silent.
    hosts.owner_gone(5, false);
    assert!(
        hosts.retirements_pending(),
        "a lost owner must remain visible to the main loop"
    );
    assert_eq!(hosts.retry_retirements(), Err(ERR_FULL));
    assert_eq!(hosts.retry_retirements(), Err(ERR_FULL), "durable");
}

#[test]
fn the_stop_hook_sequence_for_openers_and_format_plugins() {
    let pool = HostResources::try_new().unwrap();
    let fake = Fake::new(Limits::LARGE);
    let host: Rc<dyn DocHost> = fake.clone();
    pool.hosts().wire_documents(host).unwrap();
    let (opener, format) = (pool.try_owner().unwrap(), pool.try_owner().unwrap());
    let key = opener.caller("opener", AdmittedFeatures::NONE).owner;
    fake.owner.set(key);
    fake.format_owner
        .set(format.caller("format", AdmittedFeatures::NONE).owner);
    let declared = declared(&["core.document"], AdmittedFeatures::NONE);
    let id = in_frame!(opener, |scope| {
        let handle = open(&scope, &opener, &declared, "core.document").unwrap();
        *fake.answer.borrow_mut() = state_answer(0, 0);
        let input = encode("core.document", 1, &[Record::ServiceDocOpen(open_record())]);
        let reply = call(&scope, &opener, handle, op("core.document", 1), &input).unwrap();
        state_of(&reply).document
    });
    // Both hold something (D3: neither is parked again): the opener its
    // document, the format plugin the document its format reads.
    let format_key = fake.format_owner.get();
    assert_eq!(pool.hosts().owner_holds(key), Ok(true));
    assert_eq!(pool.hosts().owner_holds(format_key), Ok(true));
    assert_eq!(pool.hosts().owner_holds(u64::MAX - 7), Ok(false));
    // The format plugin stops first: the host closes the document (reason 1),
    // the opener hears it, and the stopped format is never called.
    format.release_all().unwrap();
    assert_eq!(*fake.closed.borrow(), [(id, 1)]);
    assert!(fake.images_ended.borrow().contains(&id));
    // The opener stops afterwards: still no call into the stopped format, and
    // no second event.
    opener.release_all().unwrap();
    assert!(fake.ended.borrow().is_empty());
    assert_eq!(*fake.closed.borrow(), [(id, 1)]);
    assert!(fake.table().lease(key, id).is_err(), "the id is gone");
    assert_eq!(pool.hosts().owner_holds(key), Ok(false));
    assert_eq!(pool.hosts().owner_holds(format_key), Ok(false));
}

pub(crate) struct Lib {
    pub(crate) transactions: Transactions,
    pub(crate) aborted: core::cell::RefCell<Vec<u32>>,
    /// Calls that reached the host's `state`.
    hits: core::cell::Cell<u32>,
}
impl Lib {
    pub(crate) fn new() -> Self {
        Self {
            transactions: Transactions::try_new().unwrap(),
            aborted: core::cell::RefCell::new(Vec::new()),
            hits: core::cell::Cell::new(0),
        }
    }
}
impl LibQuery for Lib {
    fn state(&self, visit: crate::native_host::lib_service::One<'_>) -> Result<(), i32> {
        self.hits.set(self.hits.get() + 1);
        visit(Record::ServiceLibState(registrations::ServiceLibState {
            state: 2,
            books: 12,
            generation: 3,
            walk_ppm: 1_000_000,
        }));
        Ok(())
    }
}
impl LibExcluded for Lib {
    fn excluded(&self, _: &str) -> bool {
        false
    }
}
impl LibWrite for Lib {
    fn transactions(&self) -> Option<&Transactions> {
        Some(&self.transactions)
    }
    fn abort_staged(&self, transaction: u32) {
        self.aborted.borrow_mut().push(transaction);
    }
}

#[test]
fn the_library_needs_the_feature_per_instance_and_its_transactions_end_with_the_owner() {
    let pool = HostResources::try_new().unwrap();
    let lib = Rc::new(Lib::new());
    pool.hosts().wire_library(lib.clone()).unwrap();
    let owner = pool.try_owner().unwrap();
    let who = owner.caller("books", AdmittedFeatures::NONE).owner;
    let state = op("core.library", 1);
    // Without the feature in the catalog the host has not wired it.
    let none = declared(&["core.library"], AdmittedFeatures::NONE);
    let not_granted = declared(&["core.library"], AdmittedFeatures::test(0, WIRED));
    let granted = declared(&["core.library"], AdmittedFeatures::test(1, WIRED));
    in_frame!(owner, |scope| {
        let input = encode("core.library", 1, &[]);
        let handle = open(&scope, &owner, &none, "core.library").unwrap();
        assert_eq!(
            call(&scope, &owner, handle, state, &input),
            Err(ERR_UNSUPPORTED)
        );
        owner.release(handle).unwrap();
        let handle = open(&scope, &owner, &not_granted, "core.library").unwrap();
        assert_eq!(call(&scope, &owner, handle, state, &input), Err(ERR_DENIED));
        owner.release(handle).unwrap();
        assert_eq!(lib.hits.get(), 0, "refused calls never reached the host");
        // Granted: the call reaches the host and its reply comes back whole.
        let handle = open(&scope, &owner, &granted, "core.library").unwrap();
        let reply = call(&scope, &owner, handle, state, &input).unwrap();
        assert_eq!(lib.hits.get(), 1);
        let Record::ServiceLibState(reply) = registrations::record_decode_one(&reply).unwrap()
        else {
            panic!("not a lib_state")
        };
        assert_eq!((reply.books, reply.generation), (12, 3));
    });
    // A transaction of the owner ends with it, after the table was released.
    let t = lib.transactions.begin_for_test(who, 0);
    let other = lib.transactions.begin_for_test(who + 1, 0);
    assert_eq!(
        pool.hosts().owner_holds(who),
        Ok(true),
        "D3: a transaction holds"
    );
    owner.release_all().unwrap();
    assert_eq!(pool.hosts().owner_holds(who), Ok(false));
    assert_eq!(pool.hosts().owner_holds(who + 1), Ok(true));
    assert_eq!(*lib.aborted.borrow(), [t]);
    assert_eq!(lib.transactions.open_count(), 1, "another owner's stays");
    let _ = other;
}

struct Dictionary;
impl LookupHost for Dictionary {
    fn ask(
        &self,
        _frame: &dyn crate::abi7::context::Frame,
        _owner: u64,
        _ask: &registrations::ServiceLookupAsk<'_>,
        _out: &mut [u8],
    ) -> Result<usize, i32> {
        Err(ERR_NOT_FOUND)
    }
}

#[test]
fn a_lookup_reaches_the_wired_providers() {
    let pool = HostResources::try_new().unwrap();
    let owner = pool.try_owner().unwrap();
    let declared = declared(&["core.lookup"], AdmittedFeatures::NONE);
    let ask = encode(
        "core.lookup",
        1,
        &[Record::ServiceLookupAsk(registrations::ServiceLookupAsk {
            text: "word",
            language: "en",
            context: "",
            cursor: 0,
        })],
    );
    in_frame!(owner, |scope| {
        let handle = open(&scope, &owner, &declared, "core.lookup").unwrap();
        let lookup = op("core.lookup", 1);
        assert_eq!(
            call(&scope, &owner, handle, lookup, &ask),
            Err(ERR_UNSUPPORTED),
            "not wired"
        );
        pool.hosts()
            .wire_lookup(Rc::new(Dictionary), Default::default())
            .unwrap();
        assert_eq!(
            call(&scope, &owner, handle, lookup, &ask),
            Err(ERR_NOT_FOUND),
            "no provider is left: the asker's loop ends"
        );
    });
}

/// Review of #308: one plugin that is both a format and an opener.
#[test]
fn a_plugin_that_is_opener_and_format_never_closes_into_itself_and_others_hear_once() {
    let pool = HostResources::try_new().unwrap();
    let fake = Fake::new(Limits::LARGE);
    let host: Rc<dyn DocHost> = fake.clone();
    pool.hosts().wire_documents(host).unwrap();
    let (both, other) = (pool.try_owner().unwrap(), pool.try_owner().unwrap());
    let both_id = both.caller("both", AdmittedFeatures::NONE).owner;
    fake.format_owner.set(both_id);
    let declared = declared(&["core.document"], AdmittedFeatures::NONE);
    // One service handle per owner; each opens as many documents as asked.
    let open_n = |owner: &resources::OwnerResources, count: usize| {
        in_frame!(owner, |scope| {
            let handle = open(&scope, owner, &declared, "core.document").unwrap();
            let mut ids = Vec::new();
            for _ in 0..count {
                *fake.answer.borrow_mut() = state_answer(0, 0);
                let input = encode("core.document", 1, &[Record::ServiceDocOpen(open_record())]);
                let reply = call(&scope, owner, handle, op("core.document", 1), &input).unwrap();
                ids.push(state_of(&reply).document);
            }
            ids
        })
    };
    let [live, host_closed] = open_n(&both, 2)[..] else {
        panic!("two documents")
    };
    let elsewhere = open_n(&other, 1)[0];
    // One of its own documents was closed by the host earlier (the card went).
    fake.table().cancel(host_closed, 2).unwrap();
    both.release_all().unwrap();
    // The format never gets a close for the plugin that is retiring, and the
    // retiring plugin hears no event; the other opener hears exactly one.
    assert!(fake.ended.borrow().is_empty());
    assert_eq!(*fake.closed.borrow(), [(elsewhere, 1)]);
    for id in [live, host_closed, elsewhere] {
        assert!(fake.images_ended.borrow().contains(&id), "{id}");
    }
    let other_id = other.caller("other", AdmittedFeatures::NONE).owner;
    assert!(fake.table().lease(both_id, live).is_err());
    assert!(fake.table().lease(both_id, host_closed).is_err());
    // The other opener still holds its (cancelled) id; stopping it calls no
    // format and sends no second event.
    assert!(
        fake.table().lease(other_id, elsewhere).is_err(),
        "cancelled"
    );
    other.release_all().unwrap();
    assert!(fake.ended.borrow().is_empty());
    assert_eq!(*fake.closed.borrow(), [(elsewhere, 1)]);
}
