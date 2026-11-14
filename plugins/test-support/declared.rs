//! Test-only: what a plugin's native declaration batches say, decoded, next to
//! what its interface-6 registration says, as lines of text that must be
//! equal. A record the host admits (groups, settings, their extras, screens,
//! actions, widgets and status declarations) must pass `validate_record`; the others are checked and then
//! refused as unsupported until the host takes them (anything else, such as
//! an invalid record, fails the test).
#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::too_many_lines,
    reason = "test support: one arm per kind of record"
)]
use paperpaw_core::native_host::declarations::validate_record;
use paperpaw_core::native_host::registrations::{Record, Registration, records_decode};
use paperpaw_sdk::{Contribution, Editor, SettingKind, SettingValue};

/// `ERR_UNSUPPORTED` of `paperpaw_native::host_glue` (a plugin has no such dependency).
const UNSUPPORTED: i32 = -1;

/// The decoded batches, as lines.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Lines {
    pub groups: Vec<String>,
    pub specs: Vec<String>,
    /// Only extras that say something.
    pub extras: Vec<String>,
    pub screens: Vec<String>,
    /// The navigation records, each as a line, sorted: slot offers
    /// (`slot|screen|priority`), places of widgets
    /// (`name|title|setting keys|layout|shown if`), places of status items,
    /// key bindings (`layer|input|action`), menu entries (`menu|item|order`),
    /// icons (`item|icon`), needs (`item|need`), actions (`name|title`) and
    /// the events subscribed to (native only: `tag:token`).
    pub slots: Vec<String>,
    pub widget_places: Vec<String>,
    pub status_places: Vec<String>,
    pub keys: Vec<String>,
    pub menu_entries: Vec<String>,
    pub icons: Vec<String>,
    pub needs: Vec<String>,
    pub actions: Vec<String>,
    pub events: Vec<String>,
    /// The kind of every record that is none of the settings, screens or
    /// these.
    pub others: Vec<String>,
}

impl Lines {
    /// Every field as a text, one line a fact, under a heading for each field.
    #[must_use]
    pub fn pinned(&self) -> String {
        let mut out = String::new();
        let sections: [(&str, &Vec<String>); 14] = [
            ("groups", &self.groups),
            ("specs", &self.specs),
            ("extras", &self.extras),
            ("screens", &self.screens),
            ("slots", &self.slots),
            ("widget_places", &self.widget_places),
            ("status_places", &self.status_places),
            ("keys", &self.keys),
            ("menu_entries", &self.menu_entries),
            ("icons", &self.icons),
            ("needs", &self.needs),
            ("actions", &self.actions),
            ("events", &self.events),
            ("others", &self.others),
        ];
        for (name, lines) in sections {
            out.push_str(&format!("[{name}]\n"));
            for line in lines {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    }
}

/// Compares what the plugin declares with the pinned text in
/// `tests/pinned/<name>.txt` of the plugin (made while the interface-6
/// registration was still compared with it, line for line, apart from the
/// differences the plugin's `PORTING-7.md` names). `PAPERPAW_BLESS=1` writes it.
pub fn assert_pinned(name: &str, lines: &Lines) {
    let path = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("tests/pinned")
        .join(format!("{name}.txt"));
    let now = lines.pinned();
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &now).unwrap();
        return;
    }
    let pinned = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} is missing: PAPERPAW_BLESS=1 makes it", path.display()));
    assert_eq!(now, pinned, "{} differs", path.display());
}

const NEEDS: [&str; 12] = [
    "Speech",
    "Sleep",
    "Rotation",
    "NoTouch",
    "Touch",
    "ButtonRow",
    "Colors",
    "Wifi",
    "Firmware",
    "BlackAndWhite",
    "Frontlight",
    "Bluetooth",
];

impl Lines {
    fn sort(mut self) -> Self {
        for list in [
            &mut self.slots,
            &mut self.widget_places,
            &mut self.status_places,
            &mut self.keys,
            &mut self.menu_entries,
            &mut self.icons,
            &mut self.needs,
            &mut self.actions,
            &mut self.events,
        ] {
            list.sort();
        }
        self
    }
}

fn default_text(bytes: &[u8]) -> String {
    let payload = &bytes[12..];
    match u32::from_le_bytes(bytes[4..8].try_into().unwrap()) {
        1 => format!("b:{}", payload[0]),
        2 => format!("i:{}", i64::from_le_bytes(payload.try_into().unwrap())),
        3 => format!("t:{}", std::str::from_utf8(payload).unwrap()),
        kind => format!("?{kind}"),
    }
}

/// The fields of a setting's extra, as either side has them.
struct Extra<'a> {
    name: &'a str,
    unit: &'a str,
    section: &'a str,
    follows: &'a str,
    shown: (&'a str, &'a str),
    editor: (u32, &'a str),
    step: i64,
    flags: u32,
}

fn extra_line(e: &Extra<'_>) -> Option<String> {
    let step = if e.step == 1 { 0 } else { e.step };
    let empty = e.unit.is_empty()
        && e.section.is_empty()
        && e.follows.is_empty()
        && e.shown.0.is_empty()
        && e.editor.0 == 0
        && step == 0
        && e.flags == 0;
    (!empty).then(|| {
        format!(
            "{}|{}|{}|{}|{}={}|{}:{}|{step}|{}",
            e.name,
            e.unit,
            e.section,
            e.follows,
            e.shown.0,
            e.shown.1,
            e.editor.0,
            e.editor.1,
            e.flags
        )
    })
}

/// Decodes `batches`, checks each record with the host's rules and returns
/// what they declare.
pub fn native(plugin: &str, batches: &[Vec<u8>]) -> Lines {
    let mut out = Lines::default();
    for batch in batches {
        let records = records_decode(batch).unwrap();
        for record in records.iter() {
            let admitted = matches!(
                record,
                Record::SettingGroup(_)
                    | Record::SettingSpec(_)
                    | Record::SettingExtra(_)
                    | Record::ScreenSpec(_)
                    | Record::ActionSpec(_)
                    | Record::WidgetSpec(_)
                    | Record::WidgetPlace(_)
                    | Record::StatusItem(_)
                    | Record::StatusLine(_)
                    | Record::StatusPlace(_)
                    | Record::Registration(_)
                    | Record::MenuEntry(_)
                    | Record::IconSpec(_)
                    | Record::MenuNeed(_)
                    | Record::MenuSection(_)
                    | Record::MenuHide(_)
                    | Record::MenuDescribe(_)
                    | Record::SlotSpec(_)
                    | Record::SlotOffer(_)
                    | Record::KeyBind(_)
                    | Record::PresetSpec(_)
                    | Record::SettingDefault(_)
                    | Record::SettingChoice(_)
                    | Record::PresetValue(_)
                    | Record::StageSpec(_)
                    | Record::RegistrationStage(_)
                    | Record::SetupStep(_)
                    | Record::ReaderSpec(_)
            );
            match validate_record(plugin, &record) {
                Ok(()) => assert!(admitted, "{record:?}"),
                Err(status) => {
                    assert_eq!(status, UNSUPPORTED, "{record:?}");
                    assert!(!admitted, "{record:?}");
                }
            }
            match record {
                Record::SettingGroup(g) => out.groups.push(format!(
                    "{}|{}|{}|{}|{}",
                    g.name, g.title, g.order, g.page, g.parent
                )),
                Record::SettingSpec(s) => out.specs.push(format!(
                    "{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}",
                    s.name,
                    s.kind,
                    s.flags,
                    s.group,
                    s.title,
                    s.help,
                    default_text(s.default),
                    s.min,
                    s.max,
                    s.choices
                )),
                Record::SettingExtra(e) => out.extras.extend(extra_line(&Extra {
                    name: e.name,
                    unit: e.unit,
                    section: e.section,
                    follows: e.follows,
                    shown: (e.shown_id, e.shown_value),
                    editor: (e.editor_kind, e.editor),
                    step: i64::from(e.step),
                    flags: e.flags,
                })),
                Record::ScreenSpec(s) => out.screens.push(format!("{}|{}", s.name, s.title)),
                Record::SlotOffer(o) => out
                    .slots
                    .push(format!("{}|{}|{}", o.slot, o.screen, o.priority)),
                Record::WidgetPlace(p) => out.widget_places.push(format!(
                    "{}|{}|{}|{}|{}={}",
                    p.name,
                    p.title,
                    p.list_keys,
                    ["Grid", "Stack"][p.layout as usize],
                    p.shown_if_key,
                    p.shown_if_value
                )),
                Record::StatusPlace(p) => out.status_places.push(format!(
                    "{}|{}|{}|{}|{}",
                    p.name, p.title, p.left, p.center, p.right
                )),
                Record::KeyBind(k) => {
                    assert!(k.when_key.is_empty(), "{k:?}");
                    out.keys
                        .push(format!("{}|{}|{}", k.layer, k.input, k.action));
                }
                Record::MenuEntry(e) => {
                    assert!(e.place == 0 && e.anchor.is_empty(), "{e:?}");
                    out.menu_entries
                        .push(format!("{}|{}|{}", e.menu, e.item, e.order));
                }
                Record::IconSpec(i) => out.icons.push(format!("{}|{}", i.item, i.icon)),
                Record::MenuNeed(n) => out
                    .needs
                    .push(format!("{}|{}", n.item, NEEDS[n.need as usize])),
                Record::ActionSpec(a) => out.actions.push(format!("{}|{}", a.name, a.title)),
                Record::Registration(Registration::Event { event_tag, token }) => {
                    out.events.push(format!("{event_tag}:{token}"));
                }
                other => {
                    let text = format!("{other:?}");
                    let kind = text.split(['(', ' ']).next().unwrap_or("");
                    out.others.push(kind.to_string());
                }
            }
        }
    }
    out.sort()
}

/// What the interface-6 registration of `plugin` says, in the same lines:
/// the group `group` (a full id) and the settings in it. An empty `group` is
/// for plugins whose settings sit in the kernel's groups (the reader): it
/// takes the settings of every group and no group record.
pub fn legacy(plugin: &str, group: &str, contributions: Vec<Contribution>) -> Lines {
    let prefix = format!("{plugin}.");
    let relative = |id: &str| id.strip_prefix(&prefix).unwrap_or(id).to_string();
    let mut out = Lines::default();
    let mut specs = Vec::new();
    let mut entries = Vec::new();
    for contribution in contributions {
        match contribution {
            Contribution::SettingGroup(g) if g.id.as_str() == group => out.groups.push(format!(
                "{}|{}|{}|{}|{}",
                relative(g.id.as_str()),
                g.title,
                g.order,
                g.page
                    .as_ref()
                    .map_or(String::new(), |p| relative(p.as_str())),
                g.parent.as_ref().map_or("", |p| p.as_str())
            )),
            Contribution::FixedSettings(list) => specs.extend(list.iter().cloned()),
            Contribution::Setting(s) => specs.push(*s),
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    out.screens
                        .push(format!("{}|{}", relative(s.id().as_str()), s.title()));
                }
            }
            Contribution::SlotBinding {
                slot,
                screen,
                priority,
                ..
            } if screen.as_str().starts_with(&prefix) => {
                out.slots.push(format!("{slot}|{screen}|{}", priority.0))
            }
            Contribution::WidgetPlace(p) if p.id.as_str().starts_with(&prefix) => {
                let keys: Vec<&str> = p.settings.iter().map(|k| k.as_str()).collect();
                let (key, value) = p
                    .shown_if
                    .as_ref()
                    .map_or(("", ""), |(id, value)| (id.as_str(), *value));
                out.widget_places.push(format!(
                    "{}|{}|{}|{:?}|{key}={value}",
                    relative(p.id.as_str()),
                    p.title,
                    keys.join("\n"),
                    p.kind
                ));
            }
            Contribution::StatusPlace(p) if p.id.as_str().starts_with(&prefix) => {
                out.status_places.push(format!(
                    "{}|{}|{}|{}|{}",
                    relative(p.id.as_str()),
                    p.title,
                    p.parts[0],
                    p.parts[1],
                    p.parts[2]
                ));
            }
            Contribution::Keymap(text) => {
                let mut layer = String::new();
                for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    if let Some(name) = line
                        .strip_prefix("[keymap.")
                        .and_then(|l| l.strip_suffix(']'))
                    {
                        layer = name.to_string();
                    } else if let Some((input, action)) = line.split_once('=') {
                        out.keys
                            .push(format!("{layer}|{}|{}", input.trim(), action.trim()));
                    }
                }
            }
            Contribution::MenuEntry(e) => entries.push(e),
            Contribution::FixedMenuEntries(list) => entries.extend(list.iter().cloned()),
            Contribution::Icon { item, icon, .. } if item.as_str().starts_with(&prefix) => {
                out.icons.push(format!("{item}|{icon}"));
            }
            Contribution::FixedIcons(list) => {
                for (item, icon) in list.iter().filter(|(i, _)| i.as_str().starts_with(&prefix)) {
                    out.icons.push(format!("{item}|{icon}"));
                }
            }
            Contribution::MenuNeed { item, need, .. } if item.as_str().starts_with(&prefix) => {
                out.needs.push(format!("{item}|{need:?}"));
            }
            Contribution::Action(a) => {
                out.actions
                    .push(format!("{}|{}", relative(a.id.as_str()), a.title))
            }
            Contribution::FixedActions(list) => {
                for a in list.iter() {
                    let a = paperpaw_sdk::Action::Fixed(a);
                    out.actions
                        .push(format!("{}|{}", relative(a.id().as_str()), a.title()));
                }
            }
            _ => {}
        }
    }
    for e in entries {
        if e.menu.as_str().starts_with(&prefix) || e.item.as_str().starts_with(&prefix) {
            assert!(e.place.is_none(), "{e:?}");
            out.menu_entries
                .push(format!("{}|{}|{}", e.menu, e.item, e.order));
        }
    }
    for s in specs {
        // An empty `group` takes the settings of every group (a plugin whose
        // settings sit in the kernel's groups).
        if !group.is_empty() && s.group.as_str() != group {
            continue;
        }
        let name = relative(s.id.as_str());
        let (kind, min, max, choices) = match &s.kind {
            SettingKind::Bool => (0, 0, 0, String::new()),
            SettingKind::Int { min, max } => (1, *min, *max, String::new()),
            SettingKind::Choice(c) => (2, 0, 0, c.iter().collect::<Vec<_>>().join("\n")),
            SettingKind::Text => (3, 0, 0, String::new()),
            SettingKind::Language => (4, 0, 0, String::new()),
            SettingKind::List => (5, 0, 0, String::new()),
            other => panic!("{other:?}"),
        };
        let e = s.extra();
        let flags = u32::from(s.scoped)
            | u32::from(s.guarded) << 1
            | u32::from(s.is_kept_on_reset()) << 2
            | u32::from(e.secret) << 3
            | u32::from(format!("{:?}", s.level) == "Advanced") << 4;
        let default = match &s.default {
            SettingValue::Bool(v) => format!("b:{}", u8::from(*v)),
            SettingValue::Int(v) => format!("i:{v}"),
            SettingValue::Text(v) => format!("t:{v}"),
            other => panic!("{other:?}"),
        };
        out.specs.push(format!(
            "{name}|{kind}|{flags}|{}|{}|{}|{default}|{min}|{max}|{choices:?}",
            s.group,
            s.title,
            s.description.as_deref().unwrap_or("")
        ));
        let editor = match &e.editor {
            None => (0, String::new()),
            Some(Editor::Screen(id)) => (1, relative(id.as_str())),
            Some(Editor::Slot(id)) => (2, id.as_str().to_string()),
            Some(other) => panic!("{other:?}"),
        };
        let flags = u32::from(e.slider)
            | u32::from(e.section_of_plugin) << 1
            | u32::from(e.keeps_unknown) << 2;
        out.extras.extend(extra_line(&Extra {
            name: &name,
            unit: e.unit.as_deref().unwrap_or(""),
            section: e.section.as_deref().unwrap_or(""),
            follows: e.follows.as_ref().map_or("", |f| f.as_str()),
            shown: e
                .shown_if
                .as_ref()
                .map_or(("", ""), |(id, value)| (id.as_str(), *value)),
            editor: (editor.0, &editor.1),
            step: e.step,
            flags,
        }));
    }
    out.sort()
}
