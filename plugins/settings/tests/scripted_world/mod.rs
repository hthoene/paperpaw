//! A scripted world for the native settings plugin: the registry (settings,
//! groups, menus, presets, widgets), `core.ui` (hand-overs, questions, texts)
//! and the plugin's texts, with the plugin made and driven by the scripted
//! host. Screen commands (redraw, open, back) are refused by the scripted host
//! (BLOCKED H1); what a callback submitted is read from its batches, which
//! keep every command, accepted or not.
#![allow(
    dead_code,
    clippy::unwrap_used,
    reason = "shared test support: each test file uses a part; assertions panic on purpose"
)]
use paperpaw_core::native_host::commands::{WireCommand, commands_decode};
use paperpaw_core::native_host::environment::{
    SNAPSHOT_BYTES, SNAPSHOT_VERSION, SettingValueRef, settings_encode,
};
use paperpaw_core::native_host::registrations::{
    EventAnswered, EventRegistryChanged, EventScreenOpened, EventScreenShown, EventSettingChanged,
    InputAction, RawRecord, Record, ServiceEnd, ServiceRegChoice, ServiceRegGroup, ServiceRegItem,
    ServiceRegKey, ServiceRegMenuRow, ServiceRegQuery, ServiceRegSetting, ServiceRegSettingExtra,
    ServiceRegText, ServiceRegValue, ServiceRegWidget, ServiceUiHandle, ServiceUiParam,
    ServiceUiRequest, ServiceUiText, records_decode,
};
use paperpaw_plugin_settings::NATIVE_ENTRY;
use paperpaw_scripted_host::{Callback, Plugin, ScriptedHost, event_batch, row};
use paperpaw_sdk::{SettingSpec, builtin::groups};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

/// The tokens of `declarations.rs`.
pub const EVENTS: u32 = 3;
pub const LIST: u32 = 10;
pub const GROUP: u32 = 11;
pub const CHOOSE: u32 = 12;
pub const THEMES: u32 = 13;
pub const SEARCH: u32 = 14;

/// `reg_setting.flags`.
pub const ADVANCED: u32 = 1 << 4;
pub const SHOWN: u32 = 1 << 9;
pub const KEPT_ON_RESET: u32 = 1 << 3;
pub const SECRET: u32 = 1 << 1;

/// A value as the registry carries it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum V {
    Bool(bool),
    Int(i64),
    Text(String),
}

fn encoded(value: Option<&V>) -> Vec<u8> {
    let value = match value {
        None => SettingValueRef::Absent,
        Some(V::Bool(v)) => SettingValueRef::Bool(*v),
        Some(V::Int(v)) => SettingValueRef::Int(*v),
        Some(V::Text(v)) => SettingValueRef::Text(v),
    };
    let mut record = [core::mem::MaybeUninit::<u8>::uninit(); 128];
    let len = settings_encode(value, &mut record).unwrap();
    // SAFETY: `settings_encode` initialized the first `len` bytes.
    record[..len]
        .iter()
        .map(|byte| unsafe { byte.assume_init() })
        .collect()
}

/// A setting of the registry.
#[derive(Clone, Debug)]
pub struct S {
    pub key: String,
    pub group: String,
    /// 0 switch, 1 number, 2 choice, 3 text, 4 language.
    pub kind: u32,
    pub flags: u32,
    /// 0 default, 3 user, 4 book scope.
    pub source: u32,
    pub title: String,
    pub value: Option<V>,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub unit: String,
    pub section: String,
    pub help: String,
    pub editor_kind: u32,
    pub editor: String,
    /// Value and name of each option of a choice.
    pub choices: Vec<(String, String)>,
}

impl S {
    pub fn new(key: &str, group: &str, kind: u32, title: &str, value: Option<V>) -> Self {
        Self {
            key: key.into(),
            group: group.into(),
            kind,
            flags: SHOWN,
            source: 0,
            title: title.into(),
            value,
            min: 0,
            max: 0,
            step: 1,
            unit: String::new(),
            section: String::new(),
            help: String::new(),
            editor_kind: 0,
            editor: String::new(),
            choices: Vec::new(),
        }
    }

    pub fn switch(key: &str, group: &str, title: &str, on: bool) -> Self {
        Self::new(key, group, 0, title, Some(V::Bool(on)))
    }

    pub fn number(key: &str, group: &str, title: &str, now: i64, range: (i32, i32)) -> Self {
        let mut s = Self::new(key, group, 1, title, Some(V::Int(now)));
        (s.min, s.max) = range;
        s
    }

    pub fn text(key: &str, group: &str, title: &str, now: &str) -> Self {
        Self::new(key, group, 3, title, Some(V::Text(now.into())))
    }

    pub fn choice(
        key: &str,
        group: &str,
        title: &str,
        now: &str,
        options: &[(&str, &str)],
    ) -> Self {
        let mut s = Self::new(key, group, 2, title, Some(V::Text(now.into())));
        s.choices = options
            .iter()
            .map(|(v, n)| ((*v).into(), (*n).into()))
            .collect();
        s
    }

    pub fn flags(mut self, flags: u32) -> Self {
        self.flags |= flags;
        self
    }

    /// Set by the user (not the default).
    pub fn set(mut self) -> Self {
        self.source = 3;
        self
    }

    pub fn help(mut self, help: &str) -> Self {
        self.help = help.into();
        self
    }

    fn record(&self) -> Vec<u8> {
        let value = encoded(self.value.as_ref());
        row(&Record::ServiceRegSetting(ServiceRegSetting {
            key: &self.key,
            group: &self.group,
            kind: self.kind,
            flags: self.flags,
            source: self.source,
            title: &self.title,
            value: &value,
        }))
        .unwrap()
    }

    fn detail(&self) -> Vec<u8> {
        let mut out = self.record();
        out.extend(
            row(&Record::ServiceRegSettingExtra(ServiceRegSettingExtra {
                min: self.min,
                max: self.max,
                step: self.step,
                unit: &self.unit,
                section: &self.section,
                follows: "",
                shown_id: "",
                shown_value: "",
                editor_kind: self.editor_kind,
                editor: &self.editor,
                choices: u32::try_from(self.choices.len()).unwrap(),
            }))
            .unwrap(),
        );
        if !self.help.is_empty() {
            out.extend(row(&Record::ServiceRegText(ServiceRegText { text: &self.help })).unwrap());
        }
        out
    }
}

/// A settings group.
#[derive(Clone, Debug)]
pub struct G {
    pub id: String,
    pub parent: String,
    pub order: i32,
    pub title: String,
}

impl G {
    pub fn new(id: &str, title: &str, order: i32) -> Self {
        Self {
            id: id.into(),
            parent: String::new(),
            order,
            title: title.into(),
        }
    }

    pub fn within(mut self, parent: &str) -> Self {
        self.parent = parent.into();
        self
    }
}

/// A row of a menu: an action, or a section heading.
#[derive(Clone, Debug)]
pub struct M {
    pub item: String,
    pub section: bool,
    pub order: i32,
    pub title: String,
}

impl M {
    pub fn action(item: &str, title: &str, order: i32) -> Self {
        Self {
            item: item.into(),
            section: false,
            order,
            title: title.into(),
        }
    }
}

/// What the registry holds.
#[derive(Clone, Debug, Default)]
pub struct Spec {
    pub settings: Vec<S>,
    pub groups: Vec<G>,
    /// By menu id.
    pub menus: BTreeMap<String, Vec<M>>,
    /// By preset kind: id and name.
    pub presets: BTreeMap<String, Vec<(String, String)>>,
    /// By preset id: setting and value (`None`: its default).
    pub preset_values: BTreeMap<String, Vec<(String, Option<V>)>>,
    pub widgets: Vec<String>,
    /// By placement id: the settings it sets itself.
    pub placements: BTreeMap<String, Vec<String>>,
}

/// One bare list reply: the rows and their end record.
fn page(rows: Vec<Vec<u8>>) -> Vec<u8> {
    let total = u32::try_from(rows.len()).unwrap();
    let mut out: Vec<u8> = rows.concat();
    out.extend(
        row(&Record::ServiceEnd(ServiceEnd {
            flags: 1,
            total,
            generation: 1,
            next: total,
            cursor: None,
        }))
        .unwrap(),
    );
    out
}

fn scope_of(input: &[u8]) -> (String, u32) {
    let query = RawRecord::single(input)
        .unwrap()
        .decode::<ServiceRegQuery<'_>>()
        .unwrap();
    (query.scope.to_owned(), query.flags)
}

/// The hand-overs by id: their params.
type Handovers = Rc<RefCell<BTreeMap<u32, Vec<(String, String)>>>>;

/// What the plugin did to the world, in order.
pub type Log = Rc<RefCell<Vec<String>>>;

/// The world the plugin runs in.
pub struct World {
    pub host: ScriptedHost,
    /// `handover k=v ...`, `ask <kind> <title> | <text> | <choices>`,
    /// `write <text>`, `list <op>` (registry list calls), `secrets list`.
    pub log: Log,
    /// What the keyboard will type (answers of text questions).
    pub typed: Rc<RefCell<String>>,
    /// The hand-overs made for the plugin's screens by the test.
    handovers: Handovers,
    /// Registry list calls (op 1, 2, 3, 4 ... each page).
    pub reads: Rc<Cell<usize>>,
    next_handover: Cell<u32>,
}

/// The environment the plugin reads: a 480 x 800 display, with or without touch.
fn snapshot(host: &ScriptedHost, touch: bool) {
    let mut bytes = vec![0_u8; SNAPSHOT_BYTES];
    bytes[0..4].copy_from_slice(&SNAPSHOT_VERSION.to_le_bytes());
    bytes[4..8].copy_from_slice(&u32::try_from(SNAPSHOT_BYTES).unwrap().to_le_bytes());
    bytes[32..34].copy_from_slice(&480_u16.to_le_bytes());
    bytes[34..36].copy_from_slice(&800_u16.to_le_bytes());
    bytes[36..38].copy_from_slice(&220_u16.to_le_bytes());
    // A panel of black and white (1 bit), with or without touch.
    bytes[42] = 1;
    bytes[48] = u8::from(touch);
    host.set_env(&bytes);
}

impl World {
    /// The world of `spec`; `advanced` is the advanced switch, `kept` the
    /// secrets the service lists (owner, name, label).
    pub fn new(spec: &Spec, advanced: bool) -> Self {
        let mut host = ScriptedHost::new();
        for s in &spec.settings {
            // The host admits the commands of settings it knows.
            let key: &'static str = Box::leak(s.key.clone().into_boxed_str());
            let title: &'static str = Box::leak(s.title.clone().into_boxed_str());
            let spec = match &s.value {
                Some(V::Bool(on)) => SettingSpec::switch(key, title, *on, groups::OTHER),
                Some(V::Int(now)) => {
                    SettingSpec::number(key, title, i64::MIN / 2, i64::MAX / 2, *now, groups::OTHER)
                }
                _ => SettingSpec::text(key, title, "", groups::OTHER),
            };
            host.settings_mut().define(spec).unwrap();
        }
        for folder in [
            concat!(env!("CARGO_MANIFEST_DIR"), "/i18n"),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../crates/paperpaw-core/i18n"
            ),
        ] {
            host.add_catalogs_in(folder).unwrap();
        }
        snapshot(&host, false);
        host.set_setting("settings.advanced", SettingValueRef::Bool(advanced));
        host.set_setting("core.developer", SettingValueRef::Bool(false));
        for s in &spec.settings {
            if let Some(value) = &s.value {
                let encoded = encoded(Some(value));
                host.set_value(&s.key, &encoded);
            }
        }
        let log = Log::default();
        let reads = Rc::new(Cell::new(0));
        let spec = Rc::new(spec.clone());
        let registry = host.registry();
        Self::registry(&registry, &spec, &log, &reads);
        let typed = Rc::new(RefCell::new(String::new()));
        let handovers = Rc::default();
        Self::ui(&host, &log, &typed, &handovers);
        Self {
            host,
            log,
            typed,
            handovers,
            reads,
            next_handover: Cell::new(100),
        }
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one closure per registry op, each a few lines"
    )]
    fn registry(
        registry: &paperpaw_scripted_host::Registry,
        spec: &Rc<Spec>,
        log: &Log,
        reads: &Rc<Cell<usize>>,
    ) {
        let counted = |op: u32, log: &Log, reads: &Rc<Cell<usize>>| {
            log.borrow_mut().push(format!("list {op}"));
            reads.set(reads.get() + 1);
        };
        let (s, l, r) = (spec.clone(), log.clone(), reads.clone());
        registry.on(1, move |_| {
            counted(1, &l, &r);
            Ok(page(s.settings.iter().map(S::record).collect()))
        });
        let s = spec.clone();
        registry.on(2, move |input| {
            let key = RawRecord::single(input)
                .unwrap()
                .decode::<ServiceRegKey<'_>>()
                .unwrap()
                .key
                .to_owned();
            s.settings
                .iter()
                .find(|x| x.key == key)
                .map(S::detail)
                .ok_or(paperpaw_native::host_glue::ERR_NOT_FOUND)
        });
        let (s, l, r) = (spec.clone(), log.clone(), reads.clone());
        registry.on(3, move |input| {
            counted(3, &l, &r);
            let (key, _) = scope_of(input);
            let rows = s
                .settings
                .iter()
                .find(|x| x.key == key)
                .map(|x| &x.choices[..])
                .unwrap_or_default()
                .iter()
                .map(|(value, title)| {
                    row(&Record::ServiceRegChoice(ServiceRegChoice { value, title })).unwrap()
                })
                .collect();
            Ok(page(rows))
        });
        let (s, l, r) = (spec.clone(), log.clone(), reads.clone());
        registry.on(4, move |_| {
            counted(4, &l, &r);
            Ok(page(
                s.groups
                    .iter()
                    .map(|g| {
                        row(&Record::ServiceRegGroup(ServiceRegGroup {
                            id: &g.id,
                            parent: &g.parent,
                            page: "",
                            order: g.order,
                            title: &g.title,
                        }))
                        .unwrap()
                    })
                    .collect(),
            ))
        });
        let (s, l, r) = (spec.clone(), log.clone(), reads.clone());
        registry.on(12, move |input| {
            counted(12, &l, &r);
            let (menu, _) = scope_of(input);
            let rows = s.menus.get(&menu).map_or_else(Vec::new, |rows| {
                rows.iter()
                    .map(|m| {
                        row(&Record::ServiceRegMenuRow(ServiceRegMenuRow {
                            item: &m.item,
                            kind: if m.section { 3 } else { 0 },
                            flags: 0,
                            order: m.order,
                            by: "",
                            title: &m.title,
                            icon: "",
                            badge: "",
                            describe: "",
                        }))
                        .unwrap()
                    })
                    .collect()
            });
            Ok(page(rows))
        });
        let (s, l, r) = (spec.clone(), log.clone(), reads.clone());
        registry.on(16, move |input| {
            counted(16, &l, &r);
            let (kind, _) = scope_of(input);
            let rows = s.presets.get(&kind).map_or_else(Vec::new, |rows| {
                rows.iter()
                    .map(|(id, title)| {
                        row(&Record::ServiceRegItem(ServiceRegItem {
                            id,
                            plugin: "themes",
                            flags: 0,
                            title,
                        }))
                        .unwrap()
                    })
                    .collect()
            });
            Ok(page(rows))
        });
        let (s, l, r) = (spec.clone(), log.clone(), reads.clone());
        registry.on(18, move |input| {
            counted(18, &l, &r);
            let (id, flags) = scope_of(input);
            let rows = if flags & 1 == 1 {
                s.placements.get(&id).map_or_else(Vec::new, |keys| {
                    keys.iter()
                        .map(|key| {
                            row(&Record::ServiceRegValue(ServiceRegValue {
                                key,
                                value: &encoded(None),
                                flags: 0,
                            }))
                            .unwrap()
                        })
                        .collect()
                })
            } else {
                s.preset_values.get(&id).map_or_else(Vec::new, |values| {
                    values
                        .iter()
                        .map(|(key, value)| {
                            row(&Record::ServiceRegValue(ServiceRegValue {
                                key,
                                value: &encoded(value.as_ref()),
                                flags: u32::from(value.is_none()),
                            }))
                            .unwrap()
                        })
                        .collect()
                })
            };
            Ok(page(rows))
        });
        let (s, l, r) = (spec.clone(), log.clone(), reads.clone());
        registry.on(19, move |_| {
            counted(19, &l, &r);
            Ok(page(
                s.widgets
                    .iter()
                    .map(|id| {
                        row(&Record::ServiceRegWidget(ServiceRegWidget {
                            id,
                            plugin: "widgets",
                            kind: "",
                            icon: "",
                            flags: 0,
                            title: id,
                            cells: 0,
                            previews: "",
                        }))
                        .unwrap()
                    })
                    .collect(),
            ))
        });
    }

    /// `core.ui`: hand-overs, questions and the texts of questions.
    fn ui(host: &ScriptedHost, log: &Log, typed: &Rc<RefCell<String>>, handovers: &Handovers) {
        let (log, typed, handovers) = (log.clone(), typed.clone(), handovers.clone());
        let next = Cell::new(1_u32);
        host.service("core.ui", 1, u16::MAX, move |op, input| {
            let batch = records_decode(input).unwrap();
            let rows: Vec<Record<'_>> = batch.iter().collect();
            match op {
                // handover
                16 => {
                    let mut made = Vec::new();
                    for record in &rows {
                        if let Record::ServiceUiParam(p) = record {
                            made.push((p.key.to_owned(), p.value.to_owned()));
                        }
                    }
                    let text: Vec<String> = made.iter().map(|(k, v)| format!("{k}={v}")).collect();
                    log.borrow_mut()
                        .push(format!("handover {}", text.join(" ")));
                    let id = 1000 + u32::try_from(handovers.borrow().len()).unwrap();
                    handovers.borrow_mut().insert(id, made);
                    Ok(row(&Record::ServiceUiHandle(ServiceUiHandle { handle: id })).unwrap())
                }
                // params
                17 => {
                    let Some(Record::ServiceUiHandle(handle)) = rows.first() else {
                        return Err(-1);
                    };
                    let made = handovers.borrow().get(&handle.handle).cloned().unwrap();
                    let mut out: Vec<Vec<u8>> = made
                        .iter()
                        .map(|(key, value)| {
                            row(&Record::ServiceUiParam(ServiceUiParam { key, value })).unwrap()
                        })
                        .collect();
                    out.push(
                        row(&Record::ServiceEnd(ServiceEnd {
                            flags: 0,
                            total: u32::try_from(made.len()).unwrap(),
                            generation: 0,
                            next: 0,
                            cursor: None,
                        }))
                        .unwrap(),
                    );
                    Ok(out.concat())
                }
                // ask
                8 => {
                    let Some(Record::ServiceUiAsk(ask)) = rows.first() else {
                        return Err(-1);
                    };
                    log.borrow_mut().push(format!(
                        "ask {} {} | {} | {}",
                        ask.kind,
                        ask.title,
                        ask.text,
                        ask.choices.replace('\n', "/")
                    ));
                    let request = next.get();
                    next.set(request + 1);
                    Ok(row(&Record::ServiceUiRequest(ServiceUiRequest { request })).unwrap())
                }
                // text_write: the text a question starts from
                9 => {
                    let Some(Record::ServiceUiText(text)) = rows.first() else {
                        return Err(-1);
                    };
                    log.borrow_mut().push(format!("write {}", text.data));
                    Ok(Vec::new())
                }
                // text_read: what was typed
                10 => {
                    let Some(Record::ServiceUiTextAt(at)) = rows.first() else {
                        return Err(-1);
                    };
                    let text = typed.borrow().clone();
                    Ok(row(&Record::ServiceUiText(ServiceUiText {
                        text: at.text,
                        offset: at.offset,
                        total: u32::try_from(text.len()).unwrap(),
                        flags: 0,
                        data: &text,
                    }))
                    .unwrap())
                }
                _ => Err(-1),
            }
        });
    }

    /// A hand-over of `params` for a screen the test opens.
    pub fn handover(&self, params: &[(&str, &str)]) -> u32 {
        let id = self.next_handover.get();
        self.next_handover.set(id + 1);
        self.handovers.borrow_mut().insert(
            id,
            params
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect(),
        );
        id
    }

    pub fn load(&self) -> Plugin<'_> {
        self.host.load(NATIVE_ENTRY.entry()).unwrap()
    }

    /// The log lines that start with `verb`.
    pub fn lines(&self, verb: &str) -> Vec<String> {
        self.log
            .borrow()
            .iter()
            .filter(|l| l.starts_with(verb))
            .cloned()
            .collect()
    }

    /// Opens `screen` (instance `instance`, on token `token`) with a hand-over
    /// of `params`, then shows it.
    pub fn open(
        &self,
        plugin: &Plugin<'_>,
        (token, screen, instance): (u32, &str, u32),
        params: &[(&str, &str)],
    ) {
        let handover = if params.is_empty() {
            0
        } else {
            self.handover(params)
        };
        let opened = event_batch(&Record::EventScreenOpened(EventScreenOpened {
            now_ms: 0,
            screen,
            instance,
            slot: "",
            flags: 0,
            handover,
            params: u32::try_from(params.len()).unwrap(),
            text_bytes: 0,
        }))
        .unwrap();
        let done = plugin.event(token, &opened).unwrap();
        assert_eq!(done.status, 0, "opening {screen}");
        shown(plugin, (token, screen, instance), (480, 800));
    }
}

/// The screen is shown (again) at `size`.
pub fn shown(plugin: &Plugin<'_>, (token, screen, instance): (u32, &str, u32), size: (u32, u32)) {
    let batch = event_batch(&Record::EventScreenShown(EventScreenShown {
        now_ms: 0,
        screen,
        width: size.0,
        height: size.1,
        instance: Some(instance),
        last_input: 0,
    }))
    .unwrap();
    assert_eq!(plugin.event(token, &batch).unwrap().status, 0);
}

/// The button `action` on the screen of `token`.
pub fn press(plugin: &Plugin<'_>, token: u32, action: &str) -> Callback {
    let batch = event_batch(&Record::InputAction(InputAction { now_ms: 0, action })).unwrap();
    plugin.input(token, &batch).unwrap()
}

/// The dialog's answer to question `request`.
pub fn answered(
    plugin: &Plugin<'_>,
    request: u32,
    (kind, choice, length): (u32, u32, usize),
) -> Callback {
    let batch = event_batch(&Record::EventAnswered(EventAnswered {
        now_ms: 0,
        request,
        kind,
        outcome: 0,
        choice,
        length: u32::try_from(length).unwrap(),
        flags: 0,
    }))
    .unwrap();
    plugin.event(EVENTS, &batch).unwrap()
}

/// The setting `key` changed.
pub fn changed(plugin: &Plugin<'_>, key: &str) -> Callback {
    let batch = event_batch(&Record::EventSettingChanged(EventSettingChanged {
        now_ms: 0,
        key,
        old: &[],
    }))
    .unwrap();
    plugin.event(EVENTS, &batch).unwrap()
}

/// The registry changed.
pub fn registry_changed(plugin: &Plugin<'_>) -> Callback {
    let batch = event_batch(&Record::EventRegistryChanged(EventRegistryChanged {
        now_ms: 0,
        what: 1,
    }))
    .unwrap();
    plugin.event(EVENTS, &batch).unwrap()
}

/// Every command the callback submitted, in order, as text: `set k=v`,
/// `reset k`, `run a`, `preset p`, `open s`, `notify t`, `redraw`, `close`.
/// Read from the batches, so commands of a callback that failed (a screen
/// command the scripted host refuses) are there too.
pub fn commands(done: &Callback) -> Vec<String> {
    let mut out = Vec::new();
    for bytes in &done.batches {
        for command in commands_decode(bytes).unwrap().iter() {
            out.push(match command {
                WireCommand::SetSetting { key, value } => match value {
                    SettingValueRef::Bool(v) => format!("set {key}={v}"),
                    SettingValueRef::Int(v) => format!("set {key}={v}"),
                    SettingValueRef::Text(v) => format!("set {key}={v}"),
                    _ => format!("set {key}"),
                },
                WireCommand::ResetSetting { key, .. } => format!("reset {key}"),
                WireCommand::RunAction { action } => format!("run {action}"),
                WireCommand::ApplyPreset { preset, .. } => format!("preset {preset}"),
                WireCommand::OpenScreen { screen } => format!("open {screen}"),
                WireCommand::Open { target, .. } => format!("open {target}"),
                WireCommand::Notify { text, .. } => format!("notify {text}"),
                WireCommand::Redraw { .. } => "redraw".to_owned(),
                WireCommand::CloseScreen => "close".to_owned(),
                other => format!("{other:?}"),
            });
        }
    }
    out
}

/// The statuses a callback ends with here: 0 and 1 (taken), and the refusal
/// of a command that needs the host's screen stack (BLOCKED H1: `open`,
/// `run_action`, `redraw`, `close`, `reset_setting` are `ERR_UNSUPPORTED`, an
/// `open` with a hand-over `ERR_INVALID`). The first command a callback
/// submits is the one that is refused, so it is the last one in its batches.
pub fn finished(status: i32) -> bool {
    matches!(
        status,
        0 | 1
            | paperpaw_native::host_glue::ERR_UNSUPPORTED
            | paperpaw_native::host_glue::ERR_INVALID
    )
}
