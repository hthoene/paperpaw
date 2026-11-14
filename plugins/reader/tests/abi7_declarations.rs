//! The native declarations are the pinned ones (`tests/pinned/*.txt`, written
//! while the interface-6 registration was still compared with them, record by
//! record). They name what the interface-6 registration named:
//! the eight settings (kinds, defaults, bounds, titles, help, groups,
//! units, steps), the six screens and the slots they fill, the 29 actions,
//! the 34 menu entries, the 13 icons, the device needs, the key bindings
//! (the two that hold while a switch is on decoded apart) and, native only,
//! the two events; the first run (the reading screen's help hints, the setup
//! step, the sample widget) is compared apart.
//! What has no native declaration yet is in `PORTING-7.md`.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_core::native_host::{
    declarations::validate_batch,
    registrations::{Record, records_decode},
};
use paperpaw_sdk::native::declare::{Declarations, ScreenDecl};

/// `lines` against `tests/pinned/<name>.txt` (`PAPERPAW_BLESS=1` writes it).
fn pinned_lines(name: &str, lines: &[String]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/pinned")
        .join(format!("{name}.txt"));
    let now = lines.join("\n") + "\n";
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let pinned = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing: PAPERPAW_BLESS=1 makes it", path.display()));
    assert_eq!(now, pinned, "{} differs", path.display());
}

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

fn all() -> Vec<Vec<u8>> {
    let mut all = Vec::new();
    all.extend(batches(|d| declarations::settings(d).unwrap()));
    all.extend(batches(|d| declarations::screens(d).unwrap()));
    all.extend(batches(|d| declarations::status(d).unwrap()));
    all.extend(batches(|d| declarations::navigation(d).unwrap()));
    all.extend(batches(|d| declarations::keys(d).unwrap()));
    all.extend(batches(|d| declarations::events(d).unwrap()));
    all
}

/// Every field of every record, decoded and checked by the host's rules, is
/// the pinned one (the status settings keep the lists' editors, the kernel's
/// `core.status-lines` and `core.status-items`, out: `PORTING-7.md`, R2b-2).
#[test]
fn the_decoded_batches_are_the_pinned_ones() {
    let native = declared::native("reader", &all());
    declared::assert_pinned("declarations", &native);
    assert_eq!(native.specs.len(), 19);
    assert_eq!(native.extras.len(), 2 + 10);
    assert_eq!(native.screens.len(), 6);
    // The instance policy, decoded: one for each set of parameters (flags 0),
    // the note and the skim bar overlays (bit 2): the page shows above them.
    let flags: Vec<u32> = batches(|d| declarations::screens(d).unwrap())
        .iter()
        .flat_map(|batch| {
            records_decode(batch)
                .unwrap()
                .iter()
                .filter_map(|r| match r {
                    Record::ScreenSpec(s) => Some(s.flags),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(flags, [0, 0, 0, 4, 4, 0]);
    assert_eq!(native.slots.len(), 6);
    assert_eq!(native.actions.len(), 29);
    assert_eq!(native.menu_entries.len(), 34);
    assert_eq!(native.icons.len(), 13);
    assert_eq!(native.needs.len(), 10);
    assert_eq!(native.keys.len(), 6);
    assert!(native.groups.is_empty() && native.widget_places.is_empty());
    assert_eq!(native.status_places.len(), 2);
    // The reading app, and the four items and the two lines of the reader,
    // declared for its lists.
    assert_eq!(
        native.others,
        [
            "ReaderSpec",
            "StatusItem",
            "StatusItem",
            "StatusItem",
            "StatusItem",
            "StatusLine",
            "StatusLine"
        ]
    );
    assert_eq!(native.events.len(), 11);
}

/// The key bindings that hold while a switch is on, decoded.
#[test]
fn the_keys_that_hold_while_a_switch_is_on_are_the_pinned_ones() {
    let mut native = Vec::new();
    for batch in batches(|d| declarations::hold_keys(d).unwrap()) {
        let records = records_decode(&batch).unwrap();
        for record in records.iter() {
            let Record::KeyBind(k) = record else {
                panic!("not a key binding: {record:?}");
            };
            assert_eq!(k.when_value, "true");
            native.push(format!(
                "{}|{}|{}|{}",
                k.layer, k.input, k.action, k.when_key
            ));
        }
    }
    assert_eq!(native.len(), 2);
    pinned_lines("hold-keys", &native);
}

/// The timer's choices are the ones the interface-6 code reads minutes from.
#[test]
fn the_timer_choices_are_the_same() {
    let native = declared::native("reader", &all());
    let timer = native
        .specs
        .iter()
        .find(|s| s.contains("aloud-timer"))
        .unwrap();
    for choice in declarations::TIMER_CHOICES {
        assert!(timer.contains(choice), "{choice} in {timer}");
    }
}

/// Every text the declarations ask for exists in both languages.
#[test]
fn the_declared_texts_have_words() {
    let (en, de) = (
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    );
    let source = include_str!("../declarations.rs");
    let mut asked = 0;
    for part in source.split('"').skip(1).step_by(2) {
        if part.starts_with("reader.") && !part.ends_with('.') && !part.contains('\n') {
            // Ids of actions, items, screens and settings are no texts.
            let is_text = en.lines().any(|l| l.starts_with(&format!("{part} = ")));
            if !is_text {
                continue;
            }
            asked += 1;
            assert!(
                de.lines().any(|l| l.starts_with(&format!("{part} = "))),
                "{part} missing in de"
            );
        }
    }
    assert!(asked >= 40, "{asked}");
}

/// The stage the reader runs after an export: the host admits its declaration, and
/// the bounds declared are the generated codec's (`stages/*.def`).
#[test]
fn the_notes_stage_is_declared_with_the_codecs_bounds() {
    use paperpaw_plugin_reader::stages::reader_notes_exported_v1 as stage;
    let mut stages = 0;
    for batch in batches(|d| declarations::stages(d).unwrap()) {
        let records = records_decode(&batch).unwrap();
        validate_batch("reader", &records).unwrap();
        for record in records.iter() {
            assert!(matches!(record, Record::StageSpec(_)), "{record:?}");
            stages += 1;
        }
    }
    assert_eq!(stages, 4);
    assert_eq!(declarations::NOTES_MAX_INPUT as usize, stage::MAX_INPUT);
    assert_eq!(declarations::NOTES_MAX_REPLY as usize, stage::MAX_REPLY);
    assert_eq!(
        declarations::NOTES_MAX_RECORDS as usize,
        stage::MAX_SUBSCRIBERS
    );
    use paperpaw_plugin_reader::stages::reader_word_looked_up_v1 as words;
    assert_eq!(declarations::WORDS_MAX_INPUT as usize, words::MAX_INPUT);
    assert_eq!(declarations::WORDS_MAX_REPLY as usize, words::MAX_REPLY);
    assert_eq!(
        declarations::WORDS_MAX_RECORDS as usize,
        words::MAX_SUBSCRIBERS
    );
    use paperpaw_plugin_reader::stages::reader_filter_v1 as filter;
    assert_eq!(declarations::FILTER_MAX_INPUT as usize, filter::MAX_INPUT);
    assert_eq!(declarations::FILTER_MAX_REPLY as usize, filter::MAX_REPLY);
    assert_eq!(
        declarations::FILTER_MAX_RECORDS as usize,
        filter::MAX_SUBSCRIBERS
    );
    use paperpaw_plugin_reader::stages::reader_decorations_v1 as marks;
    assert_eq!(
        declarations::DECORATIONS_MAX_INPUT as usize,
        marks::MAX_INPUT
    );
    assert_eq!(
        declarations::DECORATIONS_MAX_REPLY as usize,
        marks::MAX_REPLY
    );
    assert_eq!(
        declarations::DECORATIONS_MAX_RECORDS as usize,
        marks::MAX_SUBSCRIBERS
    );
}

/// The reading app: `reader.app` with the reading screen, every format, as the
/// interface-6 `ReaderSpec`. Without it the host counts no reading session.
#[test]
fn the_reading_app_is_the_one_of_interface_6() {
    let batches = batches(|d| declarations::screens(d).unwrap());
    let specs: Vec<_> = batches
        .iter()
        .flat_map(|batch| records_decode(batch).unwrap().iter().collect::<Vec<_>>())
        .filter_map(|record| match record {
            Record::ReaderSpec(spec) => Some((spec.name, spec.screen, spec.formats, spec.flags)),
            _ => None,
        })
        .map(|(n, s, f, flags)| (n.to_string(), s.to_string(), f.to_string(), flags))
        .collect();
    assert_eq!(
        specs,
        [(
            "app".to_string(),
            "reader.screen".to_string(),
            String::new(),
            0
        )]
    );
}

/// The records of the first run, each admitted by the host's rules.
fn first_run() -> Vec<Vec<u8>> {
    let batches = batches(|d| declarations::first_run(d).unwrap());
    for batch in &batches {
        validate_batch("reader", &records_decode(batch).unwrap()).unwrap();
    }
    batches
}

/// The reading screen's help hints are interface 6's (`help::add`, pinned), in
/// its order, with the same inputs and tap zones. Interface 6 also had them wait
/// for the book (`help::wait`); the native counterpart, the screen's
/// `waits_ready` bit, is refused by the host yet, so the reader cannot declare
/// it (it says the book shows already: `native_ready`). When the host admits
/// the bit, this test fails: declare it then (`PORTING-7.md`).
#[test]
fn the_help_hints_are_the_pinned_ones() {
    let waiting = batches(|d| {
        d.screen(ScreenDecl::new("screen", "reader.title", declarations::SCREEN).waits_ready())
            .unwrap();
        d.submit().unwrap();
    });
    let records = records_decode(&waiting[0]).unwrap();
    assert_eq!(validate_batch("reader", &records), Err(-1), "unsupported");
    let mut native = Vec::new();
    for batch in first_run() {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::HelpHint(h) = record {
                let zone = (h.x, h.y, h.width, h.height);
                native.push(format!("{}|{}|{zone:?}|{}", h.screen, h.input, h.text));
            }
        }
    }
    assert_eq!(native.len(), 9);
    pinned_lines("help-hints", &native);
    // Three zones side by side across the page, between the top and the bottom
    // bands (the quick panel's and the quick settings').
    assert_eq!(
        declarations::HINTS
            .iter()
            .filter_map(|(_, _, zone)| *zone)
            .map(|(x, _, w, _)| x + w)
            .collect::<Vec<_>>(),
        [333, 667, 1000]
    );
    for text in [
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    ] {
        for (key, ..) in declarations::HINTS {
            assert!(
                text.lines().any(|l| l.starts_with(&format!("{key} ="))),
                "{key}"
            );
        }
    }
}

/// The setup step asks for the kernel's text size, in interface 6's place
/// (order 30), titled as that setting is (interface 6's empty title said so;
/// a native step needs a title). It has no help and applies everywhere.
#[test]
fn the_setup_step_is_the_pinned_text_size() {
    let mut steps = Vec::new();
    for batch in first_run() {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::SetupStep(step) = record {
                steps.push(format!(
                    "reader.{}|{}|{}|{}|{}|{}|{}|{}",
                    step.name,
                    step.title,
                    step.help,
                    step.order,
                    step.page_kind,
                    step.page,
                    step.applies_key.is_empty(),
                    step.flags
                ));
            }
        }
    }
    assert_eq!(steps.len(), 1);
    pinned_lines("setup-step", &steps);
}

/// The sample widget: its id, title and kind (the default, other, pinned from
/// interface 6), previewing the reading group, with the token its draws and stage calls come
/// with; not live (only settings change it) and of no fixed size.
#[test]
fn the_sample_widget_previews_the_reading_settings() {
    let mut widgets = Vec::new();
    for batch in first_run() {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::WidgetSpec(w) = record {
                widgets.push(format!(
                    "reader.{}|{}|{}|{}|{}|{}|{}",
                    w.name, w.title, w.category, w.previews, w.cells, w.token, w.flags
                ));
            }
        }
    }
    assert_eq!(widgets.len(), 1);
    assert!(widgets[0].contains("|core.group.reading|0|"), "{widgets:?}");
    pinned_lines("sample-widget", &widgets);
}

/// Interface 6 showed the row of button hints of the reading screen only on a
/// press (`HintVisibility::OnPress`). A native screen can declare its row
/// hidden (bit 8) and no other visibility: the reader declares neither, so the
/// row shows as on any screen until the SDK has the flag (`PORTING-7.md`).
#[test]
fn the_screen_hints_on_press_are_not_declared_yet() {
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                assert_eq!(s.flags & 256, 0, "{}", s.name);
            }
        }
    }
}
