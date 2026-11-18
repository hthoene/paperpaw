//! The setup templates' rules (#72, host-free): what choosing a template does
//! (settings, switching on, queueing quietly), the offline wait for a
//! connection and its release on the next contact.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture errors fail the tests"
)]
use paperpaw_plugin_store::native_page::{Installed, Kind, Origin};
use paperpaw_plugin_store::native_queue::Queue;
use paperpaw_plugin_store::native_screen_templates::choose_template;
use paperpaw_plugin_store::native_screens::{Installs, Next, Step};
use paperpaw_plugin_store::native_template::{
    CHOSEN, PENDING, TEMPLATES, Value, chosen, find, pending, release, waiting,
};
use paperpaw_sdk::install::is_plugin_name;
use paperpaw_sdk::{MemoryStorage, Storage};

/// What the device has, as the test sets it: (id, kind, enabled).
struct Device {
    have: Vec<(&'static str, Kind, bool)>,
    can_install: bool,
    online: bool,
}

impl Installs for Device {
    fn step(&self, _: &str) -> Step {
        Step::Idle
    }
    fn queued(&self) -> bool {
        false
    }
    fn have(&self, id: &str) -> Option<Installed<'_>> {
        let (_, kind, enabled) = self.have.iter().find(|(known, ..)| *known == id)?;
        Some(Installed {
            kind: *kind,
            enabled: *enabled,
            restart_needed: false,
            version: "1.0.0",
            origin: Origin::Store,
            release: None,
            trouble: None,
        })
    }
    fn can_install(&self) -> bool {
        self.can_install
    }
    fn online(&self) -> bool {
        self.online
    }
}

fn queued(card: &MemoryStorage) -> Vec<String> {
    Queue::read(card)
        .unwrap()
        .lines()
        .iter()
        .map(|line| line.id.clone())
        .collect()
}

#[test]
fn the_table_names_plugins_and_settings_that_can_be_kept() {
    assert_eq!(TEMPLATES.len(), 4);
    for template in &TEMPLATES {
        assert_eq!(find(template.id), Some(template));
        for id in template.plugins {
            assert!(is_plugin_name(id), "{id}");
        }
        for (key, value) in template.settings {
            assert!(key.contains('.'), "{key}");
            if let Value::Text(text) = value {
                assert!(text.len() < 128);
            }
        }
    }
    assert!(find("nope").is_none());
}

#[test]
fn online_a_template_switches_on_built_ins_queues_the_rest_and_sets_its_settings() {
    let device = Device {
        have: vec![
            ("stats", Kind::BuiltIn, false),
            ("end-of-book", Kind::Code, true),
        ],
        can_install: true,
        online: true,
    };
    let card = MemoryStorage::new();
    let template = find("stats").unwrap();
    let next = choose_template(&device, &card, template).unwrap();
    assert_eq!(
        next,
        Next::Templated {
            settings: template.settings,
            switch_on: vec!["stats"],
            queued: true,
            notify: "store.template.adding",
        }
    );
    assert!(!template.settings.is_empty());
    assert_eq!(queued(&card), ["reading-goal"]);
    assert!(!card.exists(PENDING));
    assert_eq!(chosen(&card).unwrap(), Some(template));
}

#[test]
fn offline_the_plugins_wait_for_a_connection_and_the_settings_apply_at_once() {
    let device = Device {
        have: Vec::new(),
        can_install: true,
        online: false,
    };
    let card = MemoryStorage::new();
    let template = find("language").unwrap();
    let next = choose_template(&device, &card, template).unwrap();
    assert_eq!(
        next,
        Next::Templated {
            settings: template.settings,
            switch_on: Vec::new(),
            queued: false,
            notify: "store.template.later",
        }
    );
    assert!(queued(&card).is_empty());
    assert!(waiting(&card));
    assert_eq!(pending(&card).unwrap(), ["vocabulary", "anki", "translate"]);
}

#[test]
fn the_next_contact_queues_what_waited_once_and_skips_what_came_another_way() {
    let card = MemoryStorage::new();
    card.write(PENDING, b"vocabulary\nanki\n../evil\ntranslate\n")
        .unwrap();
    assert_eq!(pending(&card).unwrap(), ["vocabulary", "anki", "translate"]);
    // `anki` came another way.
    assert!(release(&card, &["vocabulary", "translate"]).unwrap());
    assert_eq!(queued(&card), ["vocabulary", "translate"]);
    assert!(!waiting(&card));
    assert!(pending(&card).unwrap().is_empty());
    // Nothing waits any more: no second queueing.
    assert!(release(&card, &[]).unwrap());
    assert_eq!(queued(&card), ["vocabulary", "translate"]);
}

#[test]
fn a_template_without_plugins_only_sets_settings() {
    let device = Device {
        have: Vec::new(),
        can_install: false,
        online: false,
    };
    let card = MemoryStorage::new();
    let template = find("minimal").unwrap();
    let next = choose_template(&device, &card, template).unwrap();
    assert_eq!(
        next,
        Next::Templated {
            settings: template.settings,
            switch_on: Vec::new(),
            queued: false,
            notify: "store.template.applied",
        }
    );
    assert_eq!(card.size(CHOSEN).unwrap(), 7);
    assert!(!waiting(&card));
}

#[test]
fn a_device_that_cannot_install_adds_only_what_it_has_built_in() {
    let device = Device {
        have: vec![("notes-vault", Kind::BuiltIn, false)],
        can_install: false,
        online: false,
    };
    let card = MemoryStorage::new();
    let next = choose_template(&device, &card, find("notes").unwrap()).unwrap();
    assert_eq!(
        next,
        Next::Templated {
            settings: &[],
            switch_on: vec!["notes-vault"],
            queued: false,
            notify: "store.template.switched-on",
        }
    );
    assert!(!waiting(&card));
}
