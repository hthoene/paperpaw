//! The native declarations name what the interface-6 registration names:
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
use paperpaw_plugin_reader::Reader;
use paperpaw_sdk::native::declare::{Declarations, ScreenDecl};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("reader"));
    Reader.register(&mut r).unwrap();
    r.into_contributions()
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
/// what the registration says.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let native = declared::native("reader", &all());
    let old = declared::legacy("reader", "", legacy());
    // The status settings (`status.rs`): the same eleven, with the lists' editors
    // left out (the kernel's `core.status-lines` and `core.status-items` screens
    // are not slots of the native host: `PORTING-7.md`, R2b-2).
    let status = |line: &&String| line.starts_with("status.");
    assert_eq!(old.specs.iter().filter(status).count(), 11);
    let unedited = |lines: &[String]| -> Vec<String> {
        let mut out: Vec<String> = lines
            .iter()
            .map(|l| {
                l.replace("|1:core.status-lines|", "|0:|")
                    .replace("|1:core.status-items|", "|0:|")
            })
            .collect();
        out.sort();
        out
    };
    assert_eq!(native.specs.len(), 19);
    assert_eq!(unedited(&native.specs), unedited(&old.specs));
    assert_eq!(unedited(&native.extras), unedited(&old.extras));
    assert_eq!(native.extras.len(), 2 + 10);
    // The screens are fixed ones: the slot offers carry the same screens.
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.screens.len(), 6);
    // The instance policy, decoded: one for each set of parameters (flags 0),
    // as every fixed screen of interface 6 is.
    for contribution in legacy() {
        if let Contribution::FixedScreens(list) = contribution {
            assert_eq!(list.len(), 6);
            assert!(
                list.iter()
                    .all(|s| s.how_many() == paperpaw_sdk::Instances::PER_PARAMS)
            );
        }
    }
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
    // The note and the skim bar are overlays (bit 2): the page shows above them.
    assert_eq!(flags, [0, 0, 0, 4, 4, 0]);
    assert_eq!(native.slots, old.slots);
    assert_eq!(native.slots.len(), 6);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.actions.len(), 29);
    // All 34 entries, also those of the kernel's items (`declared` leaves
    // out entries that name nothing of the plugin's own).
    let mut entries: Vec<String> = legacy()
        .into_iter()
        .flat_map(|c| match c {
            Contribution::FixedMenuEntries(list) => list
                .iter()
                .map(|e| format!("{}|{}|{}", e.menu, e.item, e.order))
                .collect(),
            _ => Vec::new(),
        })
        .collect();
    entries.sort();
    assert_eq!(native.menu_entries, entries);
    assert_eq!(native.menu_entries.len(), 34);
    assert!(
        old.menu_entries
            .iter()
            .all(|e| native.menu_entries.contains(e))
    );
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons.len(), 13);
    assert_eq!(native.needs, old.needs);
    assert_eq!(native.needs.len(), 10);
    assert_eq!(native.keys, old.keys);
    assert_eq!(native.keys.len(), 6);
    // Nothing else is declared in these batches, and the native side hears
    // the start, a moved book, sleep, stop and the quiet time that save the
    // place, a changed setting that lays the page out again and a changed screen
    // stack that ends reading aloud, the host's answer to a spoken sentence, a
    // document that moved in its preparation, and the store's answer for a pack
    // of letters asked for (the last eight are new in interface 7).
    assert!(native.groups.is_empty() && native.widget_places.is_empty());
    assert_eq!(native.status_places, old.status_places);
    assert_eq!(native.status_places.len(), 2);
    // The reading app (`the_reading_app_is_the_one_of_interface_6`), and the four
    // items and the two lines of the reader, declared for its lists.
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
    assert_eq!(native.events.len(), 10);
}

/// The key bindings that hold while a switch is on: the registration's
/// conditional key map, decoded.
#[test]
fn the_keys_that_hold_while_a_switch_is_on_match() {
    let mut old = Vec::new();
    for contribution in legacy() {
        if let Contribution::KeymapWhen {
            setting, source, ..
        } = contribution
        {
            let mut layer = String::new();
            for line in source.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if let Some(name) = line
                    .strip_prefix("[keymap.")
                    .and_then(|l| l.strip_suffix(']'))
                {
                    layer = name.to_string();
                } else if let Some((input, action)) = line.split_once('=') {
                    old.push(format!(
                        "{layer}|{}|{}|{}",
                        input.trim(),
                        action.trim(),
                        setting.as_str()
                    ));
                }
            }
        }
    }
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
    assert_eq!(native, old);
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
    let old = paperpaw_plugin_reader::APP;
    assert_eq!(old.as_str(), "reader.app");
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

/// The reading screen's help hints are interface 6's (`help::add`), in its
/// order, with the same inputs and tap zones. Interface 6 also had them wait
/// for the book (`help::wait`); the native counterpart, the screen's
/// `waits_ready` bit, is refused by the host yet, so the reader cannot declare
/// it (it says the book shows already: `native_ready`). When the host admits
/// the bit, this test fails: declare it then (`PORTING-7.md`).
#[test]
fn the_help_hints_are_the_ones_of_interface_6() {
    use paperpaw_sdk::help::{HelpHint, Input, POINT};
    let mut old = Vec::new();
    let mut waits = false;
    for contribution in legacy() {
        if let Contribution::ToPoint { point, text, .. } = contribution
            && point.as_str() == POINT
        {
            for line in text.lines() {
                if line == "reader.screen wait" {
                    waits = true;
                } else if let Some((
                    screen,
                    HelpHint {
                        text, input, zone, ..
                    },
                )) = HelpHint::parse(line)
                {
                    let input = match input {
                        Input::Touch => 1,
                        Input::Buttons => 2,
                        _ => 0,
                    };
                    let zone = zone.map_or((0, 0, 0, 0), |z| {
                        (
                            u32::from(z.x),
                            u32::from(z.y),
                            u32::from(z.w),
                            u32::from(z.h),
                        )
                    });
                    old.push(format!("{screen}|{input}|{zone:?}|{text}"));
                }
            }
        }
    }
    assert!(waits);
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
    assert_eq!(native, old);
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
fn the_setup_step_is_the_text_size_of_interface_6() {
    let old: Vec<_> = legacy()
        .into_iter()
        .filter_map(|c| match c {
            Contribution::SetupStep(step) => Some(step),
            _ => None,
        })
        .collect();
    let [old] = old.as_slice() else {
        panic!("one step in interface 6");
    };
    assert_eq!(old.id.as_str(), "reader.setup.font-size");
    assert_eq!((old.title.as_ref(), old.help.as_ref()), ("", ""));
    let paperpaw_sdk::setup::SetupPage::Setting(setting) = &old.page else {
        panic!("a setting's page");
    };
    let mut steps = Vec::new();
    for batch in first_run() {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::SetupStep(step) = record {
                steps.push((
                    format!("reader.{}", step.name),
                    step.title.to_string(),
                    step.help.to_string(),
                    step.order,
                    step.page_kind,
                    step.page.to_string(),
                    step.applies_key.is_empty(),
                    step.flags,
                ));
            }
        }
    }
    assert_eq!(
        steps,
        [(
            old.id.to_string(),
            "core.setting.font-size".to_string(),
            String::new(),
            old.order,
            0,
            setting.to_string(),
            true,
            1
        )]
    );
    assert_eq!(setting.as_str(), "core.font-size");
}

/// The sample widget: interface 6's id, title and kind (the default, other),
/// previewing the reading group, with the token its draws and stage calls come
/// with; not live (only settings change it) and of no fixed size.
#[test]
fn the_sample_widget_previews_the_reading_settings() {
    let old: Vec<_> = legacy()
        .into_iter()
        .filter_map(|c| match c {
            Contribution::Widget(widget) => Some(widget),
            _ => None,
        })
        .collect();
    let [old] = old.as_slice() else {
        panic!("one widget in interface 6");
    };
    let mut widgets = Vec::new();
    for batch in first_run() {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::WidgetSpec(w) = record {
                widgets.push((
                    format!("reader.{}", w.name),
                    w.title.to_string(),
                    w.category.to_string(),
                    w.previews.to_string(),
                    w.cells,
                    w.token,
                    w.flags,
                ));
            }
        }
    }
    assert_eq!(
        widgets,
        [(
            old.id.to_string(),
            old.title.to_string(),
            old.category.to_string(),
            old.previews.as_ref().unwrap().to_string(),
            0,
            declarations::SAMPLE,
            0
        )]
    );
    assert_eq!(
        old.previews.as_ref().unwrap().as_str(),
        "core.group.reading"
    );
    assert!(old.cells.is_none());
}

/// Interface 6 showed the row of button hints of the reading screen only on a
/// press (`HintVisibility::OnPress`). A native screen can declare its row
/// hidden (bit 8) and no other visibility: the reader declares neither, so the
/// row shows as on any screen until the SDK has the flag (`PORTING-7.md`).
#[test]
fn the_screen_hints_on_press_are_not_declared_yet() {
    let on_press = legacy().into_iter().any(|c| {
        matches!(
            c,
            Contribution::ScreenHints {
                ref screen,
                hints: paperpaw_sdk::HintVisibility::OnPress,
                ..
            } if screen.as_str() == "reader.screen"
        )
    });
    assert!(on_press);
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(s) = record {
                assert_eq!(s.flags & 256, 0, "{}", s.name);
            }
        }
    }
}
