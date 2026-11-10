//! The native declarations name what the interface-6 registration names:
//! the eight settings (kinds, defaults, bounds, titles, help, groups,
//! units, steps), the six screens and the slots they fill, the 29 actions,
//! the 34 menu entries, the 13 icons, the device needs, the key bindings
//! (the two that hold while a switch is on decoded apart) and, native only,
//! the two events. What has no native declaration yet is in `PORTING-7.md`.
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
use paperpaw_sdk::native::declare::Declarations;
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
    // The status lines' settings (`status.rs`) have no native declaration
    // yet (`PORTING-7.md`): left out here, and counted so that they cannot
    // be forgotten.
    let status = |line: &&String| line.starts_with("status.");
    assert!(old.specs.iter().filter(status).count() >= 10);
    let specs: Vec<String> = old.specs.iter().filter(|l| !status(l)).cloned().collect();
    let extras: Vec<String> = old.extras.iter().filter(|l| !status(l)).cloned().collect();
    assert_eq!(native.specs.len(), 8);
    assert_eq!(native.specs, specs);
    assert_eq!(native.extras, extras);
    assert_eq!(native.extras.len(), 2);
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
    // Only the note is an overlay (bit 2): the page shows above it.
    assert_eq!(flags, [0, 0, 0, 0, 4, 0]);
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
    // place, and a changed setting that lays the page out again (the last four
    // are new in interface 7).
    assert!(native.groups.is_empty() && native.widget_places.is_empty());
    assert!(native.status_places.is_empty() && native.others.is_empty());
    assert_eq!(native.events.len(), 6);
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
    assert_eq!(stages, 3);
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
}
