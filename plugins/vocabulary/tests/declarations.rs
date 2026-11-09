//! What Vocabulary declares to the host. The settings are pinned byte for byte
//! (`testdata/declarations.bin.v1`, as the build script checks them) and
//! compared field by field with interface 6 in `settings_parity.rs`; the start
//! event is checked by what it says.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "the build script also uses the builders")]
mod declarations;
use paperpaw_core::native_host::registrations::records_decode;

type Build = fn(
    &mut dyn FnMut(&[u8]) -> Result<(), i32>,
) -> Result<(), paperpaw_sdk::native::declare::DeclareError>;

fn batches(build: Build) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    build(&mut |batch: &[u8]| {
        out.push(batch.to_vec());
        Ok(())
    })
    .unwrap();
    out
}

fn records(build: Build) -> String {
    let mut all = String::new();
    for batch in batches(build) {
        for record in records_decode(&batch).unwrap().iter() {
            all.push_str(&format!("{record:?}\n"));
        }
    }
    all
}

#[test]
fn the_settings_are_the_golden() {
    let mut framed = Vec::new();
    for batch in batches(|sink| declarations::settings(sink)) {
        framed.extend_from_slice(&u32::try_from(batch.len()).unwrap().to_le_bytes());
        framed.extend_from_slice(&batch);
    }
    assert!(
        framed == include_bytes!("../testdata/declarations.bin.v1"),
        "the settings changed: read the difference and bless the golden on purpose"
    );
}

#[test]
fn the_settings_and_the_start_are_declared() {
    let all = records(|sink| declarations::settings(sink));
    for needle in [
        "collect",
        "mark",
        "new-per-day",
        "retention",
        "cards-from-lookups",
        "review-on-wake",
        "sd-auto",
        "sd-folder",
        "core.group.reading",
        "vocabulary.unit.percent",
    ] {
        assert!(all.contains(needle), "{needle} is declared: {all}");
    }
    let mut count = 0;
    for batch in batches(|sink| declarations::events(sink)) {
        count += records_decode(&batch).unwrap().len();
    }
    assert_eq!(count, 2, "the start and the minute tick");
}

#[test]
fn the_flash_cards_are_declared_with_their_screens_action_entry_and_icon() {
    let all = records(|sink| declarations::ui(sink));
    for needle in [
        "\"cards\"",
        "\"review\"",
        "vocabulary.cards.title",
        "vocabulary.review.title",
        "\"flash-cards\"",
        "core.tools",
        "vocabulary.flash-cards",
        "stack-2",
    ] {
        assert!(all.contains(needle), "{needle} is declared: {all}");
    }
    // Nothing is offered that ends in a refusal: the words' screen, the phone,
    // the SD card, the sleep card and the widget are declared with their ports.
    for absent in [
        "vocabulary.open",
        "phone",
        "sd-menu",
        "sleep-card",
        "widget",
    ] {
        assert!(!all.contains(absent), "{absent} is not declared yet: {all}");
    }
}

/// Every record is well formed for the host and admitted (`native_host/declarations.rs`: checked, then
/// `ERR_UNSUPPORTED`); they must fail only that way, and these are the only ones
/// that do.
#[test]
fn every_record_is_valid_for_the_host_or_waits_for_its_admission() {
    use paperpaw_core::native_host::declarations::validate_record;
    use paperpaw_native::host_glue::ERR_UNSUPPORTED;
    let builders: [(&str, Build); 3] = [
        ("settings", |sink| declarations::settings(sink)),
        ("events", |sink| declarations::events(sink)),
        ("ui", |sink| declarations::ui(sink)),
    ];
    let mut waiting = Vec::new();
    for (name, build) in builders {
        for batch in batches(build) {
            for record in records_decode(&batch).unwrap().iter() {
                match validate_record("vocabulary", &record) {
                    Ok(()) => {}
                    Err(ERR_UNSUPPORTED) => {
                        let text = format!("{record:?}");
                        waiting.push(format!("{name}: {}", text.split('(').next().unwrap()));
                    }
                    Err(error) => panic!("{name}: {record:?}: {error}"),
                }
            }
        }
    }
    assert!(waiting.is_empty(), "{waiting:?}");
}
