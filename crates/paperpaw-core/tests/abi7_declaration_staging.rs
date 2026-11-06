//! Host staging of declaration records: limits, uniqueness, cross-record
//! rules, whole-batch atomicity, the move at commit and allocation failure.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail the test")]
#![allow(unsafe_code, reason = "a test-only failing global allocator")]
use paperpaw_core::native_host::{
    Construction, Error, HostOwner,
    registration_scope::RegistrationScope,
    registrations::{
        EVENT_TICK_TAG, REGISTRATION_MAX_BYTES, Record, Registration, ResourceFontExtension,
        ResourceFontFace, ResourceHyphenation, SettingExtra, SettingGroup, SettingSpec,
        records_decode, records_encode, submit,
    },
    staged::{
        DECLARATIONS_MAX_BYTES, Declarations, EXTRAS_MAX, FONT_EXTENSIONS_MAX, FONT_FACES_MAX,
        GROUPS_MAX, HYPHENATIONS_MAX, SPECS_MAX,
    },
};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_UNSUPPORTED, OK};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    pin::Pin,
};

const PLUGIN: &str = "weather";

// ---- a global allocator that can fail the nth allocation of this thread ---------

thread_local! {
    static COUNTDOWN: Cell<usize> = const { Cell::new(usize::MAX) };
}

struct Failing;

fn should_fail() -> bool {
    COUNTDOWN.with(|c| match c.get() {
        usize::MAX => false,
        0 => true,
        n => {
            c.set(n - 1);
            false
        }
    })
}

// SAFETY: forwards to the system allocator, or returns null as the trait allows.
unsafe impl GlobalAlloc for Failing {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if should_fail() {
            return std::ptr::null_mut();
        }
        // SAFETY: same contract as ours.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: same contract as ours.
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if should_fail() {
            return std::ptr::null_mut();
        }
        // SAFETY: same contract as ours.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Failing = Failing;

// ---- fixtures -------------------------------------------------------------------

const TEXT_X: &[u8] = &[1, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, b'x'];

fn names(prefix: &str, count: usize) -> Vec<String> {
    (0..count).map(|n| format!("{prefix}{n}")).collect()
}

fn group<'a>(name: &'a str, parent: &'a str) -> Record<'a> {
    Record::SettingGroup(SettingGroup {
        name,
        title: "title",
        order: 1,
        page: "",
        parent,
    })
}

const BOOL_TRUE: &[u8] = &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];

/// A text spec (kind 3), or a bool spec (kind 0) when `flag` is set.
fn spec_in<'a>(name: &'a str, flag: bool, group: &'a str) -> Record<'a> {
    Record::SettingSpec(SettingSpec {
        name,
        kind: if flag { 0 } else { 3 },
        flags: 0,
        group,
        title: "title",
        help: "",
        default: if flag { BOOL_TRUE } else { TEXT_X },
        min: 0,
        max: 0,
        choices: "",
    })
}

fn spec(name: &str) -> Record<'_> {
    spec_in(name, false, "weather.main")
}

fn extra_with<'a>(name: &'a str, follows: &'a str) -> Record<'a> {
    Record::SettingExtra(SettingExtra {
        name,
        unit: "u",
        section: "s",
        follows,
        shown_id: "",
        shown_value: "",
        editor_kind: 0,
        editor: "",
        step: 1,
        flags: 0,
        check: 0,
    })
}

fn extra(name: &str) -> Record<'_> {
    extra_with(name, "")
}

fn hyphenation(language: &str) -> Record<'_> {
    Record::ResourceHyphenation(ResourceHyphenation {
        language,
        path: "hyph/a.pphp",
    })
}

fn face(name: &str) -> Record<'_> {
    Record::ResourceFontFace(ResourceFontFace {
        name,
        path: "fonts/a.ttf",
        family: "Serif",
        style: 0,
        weight: 400,
        role: "",
        priority: 0,
    })
}

fn extension(name: &str) -> Record<'_> {
    Record::ResourceFontExtension(ResourceFontExtension {
        name,
        path: "fonts/a.ppfp",
        extends: "weather.f0",
    })
}

fn draw(token: u32) -> Record<'static> {
    Record::Registration(Registration::Draw { token })
}

fn encode(records: &[Record<'_>]) -> Vec<u8> {
    let mut out = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(records, &mut out).unwrap();
    out.truncate(len);
    out
}

/// Stage `records` in batches of eight, returning the first refusal.
fn stage_all(declarations: &mut Declarations, records: &[Record<'_>]) -> Result<(), i32> {
    for chunk in records.chunks(8) {
        let bytes = encode(chunk);
        declarations.stage(PLUGIN, &records_decode(&bytes).unwrap())?;
    }
    Ok(())
}

fn stage_one(declarations: &mut Declarations, records: &[Record<'_>]) -> Result<(), i32> {
    let bytes = encode(records);
    declarations.stage(PLUGIN, &records_decode(&bytes).unwrap())
}

// ---- limits -----------------------------------------------------------------------

fn limit_case(count: usize, build: impl Fn(&[String]) -> Vec<Record<'_>>) {
    let at = names("n", count);
    let mut ok = Declarations::default();
    assert_eq!(stage_all(&mut ok, &build(&at)), Ok(()));
    let over = names("n", count + 1);
    let mut full = Declarations::default();
    let records = build(&over);
    assert_eq!(stage_all(&mut full, &records), Err(ERR_NO_SPACE));
    // The refused chunk left nothing behind: earlier whole chunks only.
    let kept = full.groups().len()
        + full.specs().len()
        + full.extras().len()
        + full.hyphenations().len()
        + full.font_faces().len()
        + full.font_extensions().len();
    assert_eq!(kept % 8, 0);
    assert!(kept <= count);
}

#[test]
fn every_count_limit_holds_at_n_and_fails_at_n_plus_one() {
    limit_case(GROUPS_MAX, |n| n.iter().map(|n| group(n, "")).collect());
    limit_case(SPECS_MAX, |n| n.iter().map(|n| spec(n)).collect());
    limit_case(HYPHENATIONS_MAX, |n| {
        n.iter().map(|n| hyphenation(n)).collect()
    });
    limit_case(FONT_FACES_MAX, |n| n.iter().map(|n| face(n)).collect());
    limit_case(FONT_EXTENSIONS_MAX, |n| {
        n.iter().map(|n| extension(n)).collect()
    });
}

#[test]
fn extras_are_capped_by_their_specs_and_by_the_byte_budget() {
    // 128 specs and 128 extras do not fit the byte budget together: the
    // struct sizes alone are about 43 KiB. Extras stop at NO_SPACE, never later.
    let specs = names("s", SPECS_MAX);
    let mut d = Declarations::default();
    let records: Vec<_> = specs.iter().map(|n| spec(n)).collect();
    stage_all(&mut d, &records).unwrap();
    let extras: Vec<_> = specs.iter().map(|n| extra(n)).collect();
    assert_eq!(EXTRAS_MAX, SPECS_MAX);
    assert_eq!(stage_all(&mut d, &extras), Err(ERR_NO_SPACE));
    assert!(!d.extras().is_empty());
    assert!(d.bytes() <= DECLARATIONS_MAX_BYTES);
    // A spec past the cap and a second extra for one spec both refuse.
    assert_eq!(stage_one(&mut d, &[spec("more")]), Err(ERR_NO_SPACE));
    assert_eq!(stage_one(&mut d, &[extra("s0")]), Err(ERR_INVALID));
    // With more room an extra for each of 64 specs does fit.
    let mut d = Declarations::default();
    stage_all(&mut d, &records[..64]).unwrap();
    stage_all(&mut d, &extras[..64]).unwrap();
    assert_eq!(d.extras().len(), 64);
}

#[test]
fn the_byte_budget_stops_staging_and_leaves_the_state_unchanged() {
    let long_title = "t".repeat(96);
    let long_help = "h".repeat(96);
    let mut text = vec![1, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0];
    text.extend(std::iter::repeat_n(b'd', 400));
    text[8..12].copy_from_slice(&400u32.to_le_bytes());
    fn make<'a>(name: &'a str, title: &'a str, help: &'a str, text: &'a [u8]) -> Record<'a> {
        Record::SettingSpec(SettingSpec {
            name,
            kind: 3,
            flags: 0,
            group: "weather.main",
            title,
            help,
            default: text,
            min: 0,
            max: 0,
            choices: "",
        })
    }
    let make = |name| make(name, &long_title, &long_help, &text);
    let all: Vec<String> = (0..SPECS_MAX).map(|n| format!("p{n:04}")).collect();
    let mut one = Declarations::default();
    stage_one(&mut one, &[make("p9999")]).unwrap();
    let cost = one.bytes();
    assert!(
        cost * SPECS_MAX > DECLARATIONS_MAX_BYTES,
        "must hit budget first"
    );
    let mut d = Declarations::default();
    let mut staged = 0;
    for name in &all {
        let before = d.clone();
        match stage_one(&mut d, &[make(name)]) {
            Ok(()) => staged += 1,
            Err(error) => {
                assert_eq!(error, ERR_NO_SPACE);
                assert_eq!(d, before);
                assert!(d.bytes() + cost > DECLARATIONS_MAX_BYTES);
                break;
            }
        }
    }
    assert_eq!(staged, DECLARATIONS_MAX_BYTES / cost);
    assert_eq!(d.bytes(), staged * cost);
    assert!(d.bytes() <= DECLARATIONS_MAX_BYTES);
}

// ---- uniqueness and extras ----------------------------------------------------------

#[test]
fn names_are_unique_per_namespace_within_and_across_batches() {
    let mut d = Declarations::default();
    stage_one(
        &mut d,
        &[
            group("a", ""),
            spec("a"),
            hyphenation("de"),
            face("f"),
            draw(1),
        ],
    )
    .unwrap();
    // Same text in different namespaces was accepted above; repeats are not.
    let before = d.clone();
    for refused in [
        vec![group("a", "")],
        vec![spec("a")],
        vec![hyphenation("DE")],
        vec![hyphenation("de")],
        vec![face("f")],
        vec![extension("f")],
        vec![group("x", ""), group("x", "")],
        vec![spec("y"), spec("y")],
        vec![hyphenation("fr"), hyphenation("FR")],
        vec![face("g"), extension("g")],
        vec![extension("g"), face("g")],
    ] {
        assert_eq!(stage_one(&mut d, &refused), Err(ERR_INVALID));
        assert_eq!(d, before);
    }
    // Faces and extensions share a namespace but other kinds do not.
    stage_one(&mut d, &[extension("a"), hyphenation("fr")]).unwrap();
}

#[test]
fn an_extra_resolves_its_spec_at_finish_and_stays_unique() {
    let mut d = Declarations::default();
    stage_one(&mut d, &[group("main", "")]).unwrap();
    stage_one(&mut d, &[extra("s")]).unwrap();
    assert_eq!(d.finish(PLUGIN), Err(ERR_INVALID));
    stage_one(&mut d, &[spec("s")]).unwrap();
    d.finish(PLUGIN).unwrap();
    assert_eq!(stage_one(&mut d, &[extra("s")]), Err(ERR_INVALID));
    assert_eq!(
        stage_one(&mut d, &[spec("t"), extra("t"), extra("t")]),
        Err(ERR_INVALID)
    );
    assert_eq!(d.specs().len(), 1);
    stage_one(&mut d, &[extra("t"), spec("t")]).unwrap();
    d.finish(PLUGIN).unwrap();
    assert_eq!(d.extras().len(), 2);
}

#[test]
fn s1_forward_extras_resolve_in_one_batch_or_a_later_batch() {
    for separate in [false, true] {
        let mut d = Declarations::default();
        stage_one(&mut d, &[group("main", "")]).unwrap();
        if separate {
            stage_one(&mut d, &[extra("s")]).unwrap();
            stage_one(&mut d, &[spec("s")]).unwrap();
        } else {
            stage_one(&mut d, &[extra("s"), spec("s")]).unwrap();
        }
        d.finish(PLUGIN).unwrap();
    }
}

#[test]
fn s1_missing_spec_is_rejected_only_when_finishing() {
    let mut d = Declarations::default();
    stage_one(&mut d, &[extra("s")]).unwrap();
    assert_eq!(d.extras().len(), 1);
    assert_eq!(d.finish(PLUGIN), Err(ERR_INVALID));
}

#[test]
fn a_per_record_violation_refuses_the_batch_unchanged() {
    let mut d = Declarations::default();
    let bad = Record::SettingGroup(SettingGroup {
        name: "Bad Name",
        title: "t",
        order: 0,
        page: "",
        parent: "",
    });
    let bytes = encode(&[spec("ok"), bad]);
    let batch = records_decode(&bytes).unwrap();
    assert_eq!(d.stage(PLUGIN, &batch), Err(ERR_INVALID));
    assert!(d.is_empty());
}

// ---- finish -------------------------------------------------------------------------

fn finished(records: &[Record<'_>]) -> Result<(), i32> {
    let mut d = Declarations::default();
    stage_one(&mut d, records).unwrap();
    d.finish(PLUGIN)
}

#[test]
fn finish_checks_own_groups_and_accepts_foreign_ones() {
    assert_eq!(finished(&[]), Ok(()));
    assert_eq!(finished(&[group("main", ""), spec("s")]), Ok(()));
    assert_eq!(finished(&[spec("s")]), Err(ERR_INVALID));
    // Another plugin whose id merely starts with ours is foreign.
    assert_eq!(finished(&[spec_in("s", false, "weather2.main")]), Ok(()));
    assert_eq!(finished(&[spec_in("s", false, "core.display")]), Ok(()));
    assert_eq!(
        finished(&[spec_in("s", false, "weather.other")]),
        Err(ERR_INVALID)
    );
    assert_eq!(finished(&[group("g", "core.display")]), Ok(()));
    assert_eq!(finished(&[group("g", "other.x")]), Ok(()));
    assert_eq!(finished(&[group("g", "weather.missing")]), Err(ERR_INVALID));
    // The group may be declared after the spec that uses it.
    assert_eq!(finished(&[spec("s"), group("main", "")]), Ok(()));
}

#[test]
fn finish_rejects_parent_cycles() {
    assert_eq!(finished(&[group("a", "weather.a")]), Err(ERR_INVALID));
    assert_eq!(
        finished(&[group("a", "weather.b"), group("b", "weather.a")]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        finished(&[
            group("a", "weather.b"),
            group("b", "weather.c"),
            group("c", "weather.a"),
            group("d", "")
        ]),
        Err(ERR_INVALID)
    );
    // A tail leading into a cycle is refused too.
    assert_eq!(
        finished(&[
            group("t", "weather.a"),
            group("a", "weather.b"),
            group("b", "weather.a")
        ]),
        Err(ERR_INVALID)
    );
    assert_eq!(
        finished(&[
            group("a", "weather.b"),
            group("b", "weather.c"),
            group("c", "core.display")
        ]),
        Ok(())
    );
}

#[test]
fn finish_requires_the_same_kind_for_an_own_follows() {
    let base = |follows| {
        [
            group("main", ""),
            spec_in("flag", true, "weather.main"),
            spec("text"),
            extra_with("text", follows),
        ]
    };
    assert_eq!(finished(&base("weather.flag")), Err(ERR_INVALID));
    assert_eq!(finished(&base("weather.text")), Ok(()));
    assert_eq!(finished(&base("core.other")), Ok(()));
    assert_eq!(finished(&base("weather.nothing")), Ok(()));
    assert_eq!(finished(&base("")), Ok(()));
}

// ---- through the construction ---------------------------------------------------------

type Context = RegistrationScope<Registration>;
type Build<'a> = Construction<'a, Context, (), Registration>;

fn new_owner() -> Pin<Box<HostOwner<Context, ()>>> {
    Box::pin(HostOwner::new(RegistrationScope::new(), ()))
}

fn send(owner: &HostOwner<Context, ()>, construction: &mut Build<'_>, bytes: &[u8]) -> i32 {
    // SAFETY: the owner stays pinned in its box for the whole test.
    let owner = unsafe { Pin::new_unchecked(owner) };
    let slot = owner.context();
    slot.scope(construction, || {
        // SAFETY: the slice is readable and unmodified for the call.
        unsafe { submit(&slot, bytes.as_ptr(), bytes.len()) }
    })
    .unwrap()
}

#[test]
fn a_plugin_declares_fifty_three_specs_over_eight_submissions_and_commit_moves_them() {
    let owner = new_owner();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let specs = names("s", 53);
    let mut records = vec![group("main", "")];
    records.extend(specs.iter().map(|n| spec(n)));
    records.push(draw(9));
    let mut batches: Vec<Vec<u8>> = records.chunks(8).map(encode).collect();
    assert_eq!(batches.len(), 7);
    batches.push(encode(&[hyphenation("de"), face("f")]));
    for bytes in &batches {
        assert_eq!(send(&owner, &mut construction, bytes), OK);
    }
    let staged = construction.declarations();
    assert_eq!(staged.specs().len(), 53);
    assert_eq!(staged.groups().len(), 1);
    assert_eq!(staged.hyphenations().len(), 1);
    assert_eq!(staged.font_faces().len(), 1);
    assert_eq!(staged.specs()[52].name, "s52");
    let expected = staged.clone();
    let name = staged.specs()[7].name.as_ptr();
    let default = staged.specs()[7].default.as_ptr();
    let list = staged.specs().as_ptr();

    let mut published = construction.commit(|_| ()).unwrap();
    assert_eq!(published.len(), 1);
    let moved = published.take_declarations();
    assert_eq!(moved, expected);
    // The same buffers, not copies.
    assert_eq!(moved.specs()[7].name.as_ptr(), name);
    assert_eq!(moved.specs()[7].default.as_ptr(), default);
    assert_eq!(moved.specs().as_ptr(), list);
    // A second take finds nothing left behind.
    assert!(published.take_declarations().is_empty());
}

#[test]
fn a_refused_batch_keeps_earlier_batches_and_poisons_the_construction() {
    let owner = new_owner();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    assert_eq!(
        send(
            &owner,
            &mut construction,
            &encode(&[group("main", ""), spec("a")])
        ),
        OK
    );
    let before = construction.declarations().clone();
    let status = send(&owner, &mut construction, &encode(&[spec("b"), spec("a")]));
    assert_eq!(status, ERR_INVALID);
    assert_eq!(construction.declarations(), &before);
    // Poisoned: a good batch is refused now, and so is commit.
    assert_ne!(send(&owner, &mut construction, &encode(&[spec("c")])), OK);
    assert_eq!(construction.declarations(), &before);
    assert!(matches!(construction.commit(|_| ()), Err(Error::Invalid)));
}

#[test]
fn a_record_failing_validation_poisons_without_staging_the_valid_ones_before_it() {
    let bad_face = Record::ResourceFontFace(ResourceFontFace {
        name: "bad",
        path: "fonts/a.ttf",
        family: "Serif",
        style: 0x8000_0000,
        weight: 400,
        role: "",
        priority: 0,
    });
    let cases: [(Vec<u8>, i32); 2] = [
        // An empty language fails the record's own check.
        (
            encode(&[group("main", ""), spec("a"), hyphenation("")]),
            ERR_INVALID,
        ),
        // An unknown style bit is refused as unsupported.
        (
            encode(&[group("main", ""), face("f"), bad_face]),
            ERR_UNSUPPORTED,
        ),
    ];
    for (bytes, expected) in cases {
        let owner = new_owner();
        let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
        assert_eq!(send(&owner, &mut construction, &bytes), expected);
        // Nothing of the batch is staged, not even the records before the bad one.
        assert!(construction.declarations().is_empty());
        let good = encode(&[group("main", ""), spec("a")]);
        assert_ne!(send(&owner, &mut construction, &good), OK);
        assert!(construction.declarations().is_empty());
        assert!(matches!(construction.commit(|_| ()), Err(Error::Invalid)));
    }
}

#[test]
fn callbacks_and_declarations_are_staged_together_or_not_at_all() {
    let owner = new_owner();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let eight: Vec<_> = (0..8).map(draw).collect();
    assert_eq!(send(&owner, &mut construction, &encode(&eight)), OK);
    // The callback slots are full: the declaration beside the draw is not kept.
    let status = send(&owner, &mut construction, &encode(&[spec("a"), draw(8)]));
    assert_eq!(status, ERR_NO_SPACE);
    assert!(construction.declarations().is_empty());
    assert!(matches!(construction.commit(|_| ()), Err(Error::Capacity)));

    // A bad declaration keeps its callbacks out too.
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let status = send(
        &owner,
        &mut construction,
        &encode(&[draw(1), spec("a"), spec("a")]),
    );
    assert_eq!(status, ERR_INVALID);
    assert!(format!("{construction:?}").contains("len: 0"));
}

#[test]
fn commit_refuses_failed_cross_record_checks_and_drops_the_declarations() {
    let owner = new_owner();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    assert_eq!(send(&owner, &mut construction, &encode(&[spec("a")])), OK);
    assert!(matches!(construction.commit(|_| ()), Err(Error::Invalid)));
    // The transaction ended: a new one starts clean.
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let ok = encode(&[group("main", ""), spec("a")]);
    assert_eq!(send(&owner, &mut construction, &ok), OK);
    assert!(construction.commit(|_| ()).is_ok());
}

#[test]
fn an_unsupported_event_registration_is_refused_whole() {
    let owner = new_owner();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let tick = Record::Registration(Registration::Event {
        event_tag: EVENT_TICK_TAG,
        token: 1,
    });
    assert_eq!(
        send(&owner, &mut construction, &encode(&[tick, spec("a")])),
        OK
    );
    let other = Record::Registration(Registration::Event {
        event_tag: EVENT_TICK_TAG + 1,
        token: 1,
    });
    assert_ne!(
        send(&owner, &mut construction, &encode(&[spec("b"), other])),
        OK
    );
    assert_eq!(construction.declarations().specs().len(), 1);
}

// ---- allocation failure -----------------------------------------------------------------

fn full_batch() -> Vec<u8> {
    encode(&[
        group("main", ""),
        spec("a"),
        extra_with("a", "core.x"),
        hyphenation("de"),
        face("f"),
        extension("e"),
    ])
}

#[test]
fn allocation_failure_at_every_step_leaves_the_declarations_unchanged() {
    let mut d = Declarations::default();
    stage_one(&mut d, &[group("first", ""), spec("p")]).unwrap();
    let bytes = full_batch();
    let batch = records_decode(&bytes).unwrap();
    let before = d.clone();
    let mut failures = 0;
    for step in 0..200 {
        let mut attempt = d.clone();
        COUNTDOWN.with(|c| c.set(step));
        let result = attempt.stage(PLUGIN, &batch);
        COUNTDOWN.with(|c| c.set(usize::MAX));
        match result {
            Ok(()) => {
                assert_eq!(attempt.groups().len(), 2);
                assert_eq!(attempt.font_extensions().len(), 1);
                assert!(attempt.finish(PLUGIN).is_ok());
                break;
            }
            Err(error) => {
                assert_eq!(error, ERR_NO_MEMORY);
                assert_eq!(attempt, before);
                failures += 1;
            }
        }
    }
    // Six reserves, thirty-odd copies and six more reserves in self all fail.
    assert!(failures > 20, "only {failures} failure points exercised");
    assert_eq!(d, before);
}

#[test]
fn the_construction_latches_the_cause_of_a_refusal() {
    // A limit: specs until a count or the byte budget refuses one batch.
    let owner = new_owner();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let all = names("s", 8 * 64);
    let mut status = OK;
    for chunk in all.chunks(8) {
        let records: Vec<_> = chunk.iter().map(|name| spec(name)).collect();
        status = send(&owner, &mut construction, &encode(&records));
        if status != OK {
            break;
        }
    }
    assert_eq!(status, ERR_NO_SPACE);
    assert!(matches!(construction.commit(|_| ()), Err(Error::Capacity)));

    // Low memory: the first allocation of staging fails.
    let owner = new_owner();
    let mut construction = owner.as_ref().begin(PLUGIN).unwrap();
    let bytes = encode(&[spec("a")]);
    COUNTDOWN.with(|c| c.set(0));
    let status = send(&owner, &mut construction, &bytes);
    COUNTDOWN.with(|c| c.set(usize::MAX));
    assert_eq!(status, ERR_NO_MEMORY);
    assert!(matches!(construction.commit(|_| ()), Err(Error::Memory)));
}

// ---- mutation ---------------------------------------------------------------------------

#[test]
fn mutated_valid_batches_never_panic() {
    let valid = full_batch();
    let mut accepted = 0;
    for index in 0..valid.len() {
        for flip in [0x01u8, 0x80, 0xff, 0x20] {
            let mut bytes = valid.clone();
            bytes[index] ^= flip;
            let Ok(batch) = records_decode(&bytes) else {
                continue;
            };
            let mut d = Declarations::default();
            if d.stage(PLUGIN, &batch).is_ok() {
                accepted += 1;
                let _ = d.finish(PLUGIN);
                // Staging again must hit the uniqueness rules, not panic.
                let _ = d.stage(PLUGIN, &batch);
            }
        }
    }
    assert!(accepted > 0);
    for length in 0..valid.len() {
        if let Ok(batch) = records_decode(&valid[..length]) {
            let _ = Declarations::default().stage(PLUGIN, &batch);
        }
    }
}
