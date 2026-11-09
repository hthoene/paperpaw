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
    assert_eq!(
        count, 3,
        "the start, the minute tick and the work of the card files"
    );
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
        "\"list\"",
        "\"sd-menu\"",
        "\"sd-card\"",
        "vocabulary.sd.title",
        "\"open\"",
        "vocabulary.open",
        "vocabulary.title",
        "core.tools",
        "vocabulary.flash-cards",
        "stack-2",
    ] {
        assert!(all.contains(needle), "{needle} is declared: {all}");
    }
    // Nothing is offered that ends in a refusal: the phone,
    // the sleep card are declared with their ports.
    for absent in ["phone", "sleep-card", "widget", "scan-card"] {
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
    let builders: [(&str, Build); 5] = [
        ("widgets", |sink| declarations::widgets(sink)),
        ("stages", |sink| declarations::stages(sink)),
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

/// The three widgets, each with its group of settings: the look they share
/// (seven settings), the size and the alignment, and what each has of its own.
#[test]
fn the_widgets_are_declared_with_their_settings() {
    let all = records(|sink| declarations::widgets(sink));
    for widget in ["widget.word", "widget.saved", "widget.due"] {
        assert!(all.contains(widget), "{widget}: {all}");
    }
    assert!(all.contains("core.widgets.words"), "{all}");
    // The settings of interface 6, by name (the plugin's id is put before it).
    for key in [
        "widget.word.pick",
        "widget.word.sentence",
        "widget.word.book",
        "widget.saved.newest",
        "widget.saved.books",
        "widget.saved.languages",
        "widget.due.caption",
    ] {
        assert!(all.contains(&format!("name: \"{key}\"")), "{key}: {all}");
    }
}

/// A widget has the settings the shared look gives it, and its own, in the
/// order interface 6 listed them.
#[test]
fn every_widget_has_the_shared_look_then_its_own() {
    for widget in declarations::WIDGETS {
        let kinds: Vec<_> = widget.all().collect();
        assert_eq!(kinds.len(), 7 + widget.options.len(), "{}", widget.name);
    }
    let counts: Vec<_> = declarations::WIDGETS
        .iter()
        .map(|widget| widget.all().count())
        .collect();
    // Interface 6: 13, 13 and 10 settings (the group is not a setting).
    assert_eq!(counts, [13, 13, 10]);
}
