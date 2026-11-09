//! The native declarations name what the interface-6 registration names: the
//! settings (ids, kinds, defaults, choices, titles, help), the group, the
//! screens, the slot, the action with its key binding, and the first-run step.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar, SettingKind};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("keyboard"));
    Keyboard.register(&mut r).unwrap();
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

#[test]
fn settings_match_the_interface_6_registration() {
    let mut found = Vec::new();
    for contribution in legacy() {
        if let Contribution::FixedSettings(settings) = contribution {
            for spec in settings.iter() {
                found.push((
                    spec.id.as_str().to_string(),
                    spec.title.to_string(),
                    spec.description.as_deref().map(str::to_string),
                    spec.group.as_str().to_string(),
                    matches!(spec.kind, SettingKind::List),
                ));
            }
        }
    }
    found.sort();
    let mut expected: Vec<(String, String, Option<String>, String, bool)> = [
        ("auto-capitals", false),
        ("key-shape", false),
        ("layouts", true),
        ("size", false),
        ("umlaut-keys", false),
    ]
    .iter()
    .map(|(name, list)| {
        (
            format!("keyboard.{name}"),
            format!("keyboard.setting.{name}"),
            Some(format!("keyboard.setting.{name}.help")),
            "keyboard.group".to_string(),
            *list,
        )
    })
    .collect();
    expected.sort();
    assert_eq!(found, expected);
    // The native declarations encode without a refusal, in one batch.
    assert_eq!(batches(|d| declarations::settings(d).unwrap()).len(), 1);
}

#[test]
fn screens_slot_and_action_match() {
    let mut screens = Vec::new();
    let mut slots = Vec::new();
    let mut actions = Vec::new();
    let mut keymaps = Vec::new();
    for contribution in legacy() {
        match contribution {
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    screens.push((s.id().as_str().to_string(), s.title().to_string()));
                }
            }
            Contribution::Screen(s) => {
                screens.push((s.id.as_str().to_string(), s.title.to_string()));
            }
            Contribution::SlotBinding { slot, screen, .. } => {
                slots.push((slot.to_string(), screen.to_string()));
            }
            Contribution::Action(spec) => actions.push(spec.id.as_str().to_string()),
            Contribution::Keymap(text) => keymaps.push(text.to_string()),
            _ => {}
        }
    }
    screens.sort();
    assert_eq!(
        screens,
        [
            (
                "keyboard.choose".to_string(),
                "keyboard.setting.layouts".to_string()
            ),
            ("keyboard.screen".to_string(), "keyboard.title".to_string()),
        ]
    );
    assert_eq!(
        slots,
        [("core.keyboard".to_string(), "keyboard.screen".to_string())]
    );
    assert_eq!(actions, ["keyboard.done"]);
    assert!(
        keymaps
            .iter()
            .any(|k| k.contains("[keymap.keyboard.screen]")
                && k.contains("long.back = keyboard.done"))
    );
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert_eq!(batches(|d| declarations::navigation(d).unwrap()).len(), 1);
}

#[test]
fn the_plugin_and_the_declarations_share_their_tokens() {
    use paperpaw_plugin_keyboard::native_plugin as plugin;
    assert_eq!(
        [plugin::SCREEN, plugin::CHOOSE, plugin::DONE],
        [
            declarations::SCREEN,
            declarations::CHOOSE,
            declarations::DONE
        ]
    );
}

/// The stage other plugins add layouts through is declared, and its bounds are
/// those of the generated codec (`stages/keyboard.layouts@1.def`).
#[test]
fn the_layouts_stage_is_declared_with_the_codecs_bounds() {
    use paperpaw_core::native_host::{
        declarations::validate_batch,
        registrations::{Record, records_decode},
    };
    let mut stages = 0;
    for batch in batches(|d| declarations::stages(d).unwrap()) {
        let records = records_decode(&batch).unwrap();
        validate_batch("keyboard", &records).unwrap();
        for record in records.iter() {
            let Record::StageSpec(spec) = record else {
                panic!("an unexpected record: {record:?}")
            };
            assert_eq!(spec.name, "layouts");
            stages += 1;
        }
    }
    assert_eq!(stages, 1);
    let schema: serde_json_free::Def =
        serde_json_free::read(include_str!("../stages/keyboard.layouts@1.def"));
    assert_eq!(
        (
            schema.max_input,
            schema.max_reply,
            schema.reply_rows,
            schema.major
        ),
        (
            declarations::LAYOUTS_MAX_INPUT,
            declarations::LAYOUTS_MAX_REPLY,
            declarations::LAYOUTS_MAX_RECORDS,
            declarations::LAYOUTS_SCHEMA
        )
    );
}

/// The few numbers of a schema file, read without a JSON library.
mod serde_json_free {
    pub struct Def {
        pub max_input: u32,
        pub max_reply: u32,
        pub reply_rows: u32,
        pub major: u32,
    }

    fn number(text: &str, key: &str) -> u32 {
        let at = text.find(&format!("\"{key}\":")).unwrap() + key.len() + 3;
        text[at..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap()
    }

    pub fn read(text: &str) -> Def {
        Def {
            max_input: number(text, "max_input"),
            max_reply: number(text, "max_reply"),
            reply_rows: number(text, "reply_rows"),
            major: number(text, "major"),
        }
    }
}
