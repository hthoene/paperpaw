//! The flat Rules plugin: the rules run on the host's events and ask for actions,
//! settings and messages through commands; the list of rules, its dialogs and the
//! choice of an action or a setting are the plugin's screens. Time of day and
//! "every so many minutes" follow the host's minute tick (`event.tick`).
use crate::native_data::{Named, Reader, Setting, Sort};
use crate::native_rules::{
    engine::{self, Act, Battery, Chain, Facts, MAX_CHAIN, Value},
    rule::{
        self, Condition, EVERY_DAY, Effect, MAX_FILE, Rule, Trigger, WEEKDAYS, WEEKENDS,
        copy_rules, parse_percent, parse_time,
    },
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered, BookKind, Event, Incoming, kinds},
        formatting::Arg,
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        snapshot,
        storage::Root,
        ui::{Instance, Notify, Open, Question, QuestionKind, RequestId, TextMode, Ui},
    },
    time::{TIME_ZONES, TimeZone},
};
use paperpaw_ui_kit::read_event;
use paperpaw_ui_kit::{
    Footer, Item, ItemList, ItemStep, Messages, Screens, files,
    mem::{SERVICE_BUFFER, buffer, push},
    text::{concat, copy},
    uitext,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/rules_declarations.rs"));
}

/// Callback tokens, as `declarations.rs` gives them.
const OPEN: u32 = 1;
const LIST: u32 = 20;
const PICK: u32 = 21;

const FILE: &str = "rules.txt";
/// The most a typed answer may be.
const ANSWER_LIMIT: usize = 256;
/// The triggers a rule is started from, in the order they are offered.
const TRIGGERS: [&str; 14] = [
    "start",
    "time",
    "every",
    "book",
    "book-closed",
    "book-finished",
    "resume",
    "sleep",
    "battery",
    "battery-at",
    "charging",
    "unplugged",
    "switch-on",
    "switch-off",
];
/// What a rule can do, in the order offered.
const EFFECTS: [&str; 5] = ["action", "switch-on", "switch-off", "number", "message"];
/// The conditions, in the order offered.
const CONDITIONS: [&str; 8] = [
    "always",
    "between",
    "reading",
    "not-reading",
    "battery",
    "charging",
    "switch-on",
    "switch-off",
];

/// What an answer is for.
#[derive(Debug)]
enum Purpose {
    When,
    TimeText,
    Days(u16),
    EveryText,
    BatteryText,
    Level,
    LevelText,
    What,
    NumberText(String, i64, i64),
    MessageText,
    Option(usize),
    Condition(usize),
    FromText(usize),
    ToText(usize, u16),
}

/// What a pick of an action or setting is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickFor {
    /// The setting a switch trigger waits for.
    Trigger(bool),
    EffectAction,
    EffectSwitch(bool),
    EffectNumber,
    /// The switch a condition looks at: the rule, and on or off.
    Condition(usize, bool),
}

/// The rules on the card and the lines this version does not read.
type Card = (Vec<Rule>, Vec<u8>);

/// What the plugin shows.
#[derive(Debug)]
enum View {
    List(ListView),
    Pick(PickView),
}

#[derive(Debug)]
struct ListView {
    list: ItemList,
    /// How many rows are rules (the next one adds).
    rules: usize,
    empty: bool,
}

#[derive(Debug)]
struct PickView {
    list: ItemList,
    title: String,
    choices: Vec<String>,
}

/// The interface-7 Rules plugin.
#[derive(Debug, Default)]
pub struct NativeRules {
    ui: Option<Ui>,
    /// The rules, read once and kept; `None` while the card cannot be read.
    rules: Option<Vec<Rule>>,
    /// The lines of the file this version does not read.
    unread: Vec<u8>,
    reading: bool,
    chain: Chain,
    battery: Battery,
    /// Minutes since the start, for the rules that repeat.
    minutes: u32,
    /// The last minute (since 1970) rules of a time of day were run for.
    last_minute: Option<i64>,
    screens: Screens<View>,
    asked: Option<(RequestId, Purpose)>,
    /// The trigger of the rule being added.
    trigger: Option<Trigger>,
    pick: Option<PickFor>,
}

impl Plugin for NativeRules {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::ActionRun, kinds::Started, kinds::Resume, kinds::Suspend, kinds::Tick, kinds::BookOpened, kinds::BookClosed, kinds::BookFinished, kinds::StatusChanged, kinds::SettingChanged, kinds::ScreenOpened, kinds::ScreenShown, kinds::ScreenClosed, kinds::Answered)
        else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("rules.list")?)
            }
            Event::Started { .. } => self.fire(host, |t| *t == Trigger::Start),
            Event::Resume { .. } => self.fire(host, |t| *t == Trigger::Resume),
            Event::Suspend { .. } => self.fire(host, |t| *t == Trigger::Sleep),
            Event::Tick { .. } => self.tick(host),
            Event::Book { kind, .. } => self.book(host, kind),
            Event::StatusChanged { .. } => self.status(host),
            Event::SettingChanged { key, .. } => self.setting_changed(host, key),
            _ => self.ui_event(host, event),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        match self.screens.top(token).ok_or(Error::State)? {
            View::List(view) => {
                let title = messages.copy(host, "rules.title")?;
                let hint = messages.copy(host, "rules.hint")?;
                let heading = messages.copy(host, "rules.empty.title")?;
                let text = messages.copy(host, "rules.empty")?;
                let footer = if view.empty {
                    Footer::Empty {
                        title: &heading,
                        text: &text,
                    }
                } else {
                    Footer::Note(&hint)
                };
                view.list.draw_with(host, &mut frame, &title, true, footer)
            }
            View::Pick(view) => view.list.draw(host, &mut frame, &view.title, true),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let list = match view {
            View::List(v) => &mut v.list,
            View::Pick(v) => &mut v.list,
        };
        let step = list.input(host, &input)?;
        match step {
            ItemStep::Chosen(index) => self.chosen(host, token, index)?,
            ItemStep::Held(index) => self.held(host, token, index)?,
            _ => {}
        }
        Ok(ItemList::handled(step))
    }
}

/// What the host says of a setting now: a switch or a number; `None` for
/// anything else, or where it has no value.
fn read_value(host: &Host<'_>, id: &str) -> Result<Option<Value>, Error> {
    let mut scratch = [0; 256];
    match settings::read_setting(host, id, &mut scratch) {
        Ok(SettingValue::Bool(v)) => Ok(Some(Value::Bool(v))),
        Ok(SettingValue::Int(v)) => Ok(Some(Value::Int(v))),
        Ok(_) | Err(Error::NotFound | Error::Denied | Error::NoSpace | Error::Invalid) => Ok(None),
        Err(error) => Err(error),
    }
}

/// The seconds the zone the reader sees is ahead of UTC at `unix`: the rule
/// typed in settings, else the zone chosen from the list, else ("automatic",
/// the default) the system's, `system` minutes as the snapshot carries it
/// (as Beta 3's `time::zone_offset`, #257).
fn zone_offset(host: &Host<'_>, unix: i64, system: Option<i16>) -> i32 {
    let mut scratch = [0; 128];
    let text = |key: &str, scratch: &mut [u8; 128]| -> Option<String> {
        match settings::read_setting(host, key, scratch) {
            Ok(SettingValue::Text(t)) => copy(t).ok(),
            _ => None,
        }
    };
    if let Some(zone) = text("core.time-zone-rule", &mut scratch).and_then(|r| TimeZone::parse(&r))
    {
        return zone.offset_at(unix);
    }
    let chosen = text("core.time-zone", &mut scratch).and_then(|id| {
        TIME_ZONES
            .iter()
            .find(|(zone, _)| *zone == id)
            .and_then(|(_, rule)| TimeZone::parse(rule))
    });
    chosen_or_system(chosen.map(|zone| zone.offset_at(unix)), system)
}

/// The chosen zone's offset in seconds, else the system's (`system` minutes
/// ahead of UTC), else UTC.
fn chosen_or_system(chosen: Option<i32>, system: Option<i16>) -> i32 {
    chosen.unwrap_or_else(|| system.map_or(0, |minutes| i32::from(minutes) * 60))
}

impl NativeRules {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    // ---- the rules on the card

    /// The card's rules and the lines it holds that this version does not read:
    /// none where it cannot be read (then nothing is saved over it).
    fn read_card(host: &Host<'_>) -> Result<Option<Card>, Error> {
        match files::read_all(host, Root::Data, FILE, MAX_FILE) {
            Ok(Some(bytes)) => Ok(Some((rule::decode(&bytes)?, rule::unread_lines(&bytes)?))),
            Ok(None) => Ok(Some((Vec::new(), Vec::new()))),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => Ok(None),
        }
    }

    /// Reads the rules once and keeps them; tried again next time while the card
    /// cannot be read.
    fn load_rules(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.rules.is_none()
            && let Some((rules, unread)) = Self::read_card(host)?
        {
            self.rules = Some(rules);
            self.unread = unread;
        }
        Ok(())
    }

    /// Keeps `rules` on the card with the lines this version does not read;
    /// whether it could. Where it could not, what the card has is read again
    /// next time.
    fn save(&mut self, host: &Host<'_>, rules: Vec<Rule>) -> Result<bool, Error> {
        let saved = match Self::read_card(host)? {
            None => None,
            Some((_, unread)) => match rule::encode_within(&rules, &unread, MAX_FILE) {
                Ok(bytes) => files::replace(host, Root::Data, FILE, &bytes)
                    .is_ok()
                    .then_some(unread),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                Err(_) => None,
            },
        };
        if let Some(unread) = saved {
            self.rules = Some(rules);
            self.unread = unread;
            Ok(true)
        } else {
            self.rules = None;
            let text = Messages::new()?.copy(host, "rules.not-saved")?;
            self.notify(host, &text)?;
            Ok(false)
        }
    }

    fn notify(&self, host: &Host<'_>, text: &str) -> Result<(), Error> {
        let mut scratch = buffer(SERVICE_BUFFER)?;
        CommandSender::new(host, &mut scratch).notify(Notify::new(text)?)
    }

    // ---- the rules at work

    /// Runs the rules whose trigger `applies`.
    fn fire(&mut self, host: &Host<'_>, applies: impl Fn(&Trigger) -> bool) -> Result<(), Error> {
        self.fire_at(host, 0, &applies)
    }

    fn fire_at(
        &mut self,
        host: &Host<'_>,
        depth: u8,
        applies: &dyn Fn(&Trigger) -> bool,
    ) -> Result<(), Error> {
        self.load_rules(host)?;
        let Some(rules) = self.rules.as_deref() else {
            return Ok(());
        };
        if !rules.iter().any(|r| r.on && applies(&r.trigger)) {
            return Ok(());
        }
        let snapshot = snapshot::capture_snapshot(host)?;
        let minute = snapshot.unix_seconds().map(|unix| {
            let offset = zone_offset(host, unix, snapshot.utc_offset_minutes());
            engine::local_minute(unix, offset).0
        });
        let setting = |id: &str| read_value(host, id);
        let facts = Facts {
            reading: self.reading,
            charging: snapshot.battery_percent().map(|_| snapshot.is_charging()),
            minute,
            setting: &setting,
        };
        let mut acts = Vec::new();
        engine::run(rules, applies, &facts, depth, &mut self.chain, &mut acts)?;
        Self::apply(host, &acts)
    }

    /// Does what the rules asked. A refused command does not stop the others.
    fn apply(host: &Host<'_>, acts: &[Act]) -> Result<(), Error> {
        if acts.is_empty() {
            return Ok(());
        }
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let mut commands = CommandSender::new(host, &mut scratch);
        for act in acts {
            let result = match act {
                Act::Run(action) => commands.run_action(action),
                Act::Announce(text) => {
                    Notify::new(text).and_then(|n| commands.notify(n.announce()))
                }
                Act::Set(id, Value::Bool(v)) => commands.set_setting(id, SettingValue::Bool(*v)),
                Act::Set(id, Value::Int(v)) => commands.set_setting(id, SettingValue::Int(*v)),
            };
            if result == Err(Error::NoMemory) {
                return result;
            }
        }
        Ok(())
    }

    /// The minute tick: rules that repeat, and those waiting for a time of day.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.minutes = self.minutes.saturating_add(1);
        let minutes = self.minutes;
        self.fire(host, |t| engine::every_due(t, minutes))?;
        self.load_rules(host)?;
        if !self.rules.as_deref().is_some_and(|r| {
            r.iter()
                .any(|r| r.on && matches!(r.trigger, Trigger::Time { .. }))
        }) {
            return Ok(());
        }
        let snapshot = snapshot::capture_snapshot(host)?;
        let Some(unix) = snapshot.unix_seconds() else {
            return Ok(());
        };
        let offset = zone_offset(host, unix, snapshot.utc_offset_minutes());
        let (minute, weekday) = engine::local_minute(unix, offset);
        let stamp = unix.div_euclid(60);
        if self.last_minute == Some(stamp) {
            return Ok(());
        }
        self.last_minute = Some(stamp);
        self.fire(host, |t| engine::time_due(t, minute, weekday))
    }

    fn book(&mut self, host: &Host<'_>, kind: BookKind) -> Result<(), Error> {
        match kind {
            BookKind::Opened => {
                self.reading = true;
                self.fire(host, |t| *t == Trigger::BookOpened)
            }
            BookKind::Closed => {
                self.reading = false;
                self.fire(host, |t| *t == Trigger::BookClosed)
            }
            _ => self.fire(host, |t| *t == Trigger::BookFinished),
        }
    }

    /// The battery or charging changed: compare with how it was.
    fn status(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let snapshot = snapshot::capture_snapshot(host)?;
        let Some(percent) = snapshot.battery_percent() else {
            return Ok(());
        };
        match self.battery.look(percent, snapshot.is_charging()) {
            Some(change) => self.fire(host, |t| change.sets_off(t)),
            None => Ok(()),
        }
    }

    fn setting_changed(&mut self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        if key.is_empty() {
            return Ok(());
        }
        let depth = self.chain.depth_of(key);
        let Some(Value::Bool(on)) = read_value(host, key)? else {
            return Ok(());
        };
        // Rules undoing each other's changes stop after a few.
        if depth >= MAX_CHAIN {
            return Ok(());
        }
        self.fire_at(host, depth, &|t| engine::switch_due(t, key, on))
    }
}

/// The titles the rows say things with: settings and actions by id.
struct Words {
    messages: Messages,
    settings: Vec<Setting>,
    actions: Vec<Named>,
}

impl Words {
    /// Reads the registry's names only where a rule refers to some.
    fn new(host: &Host<'_>, rules: &[Rule]) -> Result<Self, Error> {
        let messages = Messages::new()?;
        let refers = rules.iter().any(|r| {
            matches!(r.trigger, Trigger::SwitchOn(_) | Trigger::SwitchOff(_))
                || matches!(
                    r.effect,
                    Effect::Action(_) | Effect::Switch(..) | Effect::Number(..)
                )
                || matches!(
                    r.condition,
                    Condition::SwitchOn(_) | Condition::SwitchOff(_)
                )
        });
        let (mut settings, mut actions) = (Vec::new(), Vec::new());
        if refers {
            let mut reader = Reader::open(host)?;
            let found = (|| Ok((reader.settings()?, reader.actions()?)))();
            (settings, actions) = reader.finish(found)?;
        }
        Ok(Self {
            messages,
            settings,
            actions,
        })
    }

    fn setting(&self, id: &str) -> Result<String, Error> {
        copy(
            self.settings
                .iter()
                .find(|s| s.id == id)
                .map_or(id, |s| s.title.as_str()),
        )
    }

    fn clock(&mut self, host: &Host<'_>, minute: u16) -> Result<String, Error> {
        let mut buf = [0; 5];
        let text = rule::clock(minute, &mut buf);
        self.messages.digits(host, text)
    }

    fn days(&mut self, host: &Host<'_>, days: u8) -> Result<String, Error> {
        let key = match days {
            EVERY_DAY => "rules.days.every",
            WEEKDAYS => "rules.days.weekdays",
            WEEKENDS => "rules.days.weekends",
            _ => {
                let mut out = String::new();
                for day in 0..7u8 {
                    if days & (1 << day) == 0 {
                        continue;
                    }
                    let mut digits = [0; 20];
                    let key = concat(&[
                        "rules.days.",
                        paperpaw_ui_kit::text::decimal(usize::from(day), &mut digits),
                    ])?;
                    let name = self.messages.copy(host, &key)?;
                    let extra = if out.is_empty() { 0 } else { 2 };
                    out.try_reserve(name.len() + extra)
                        .map_err(|_| Error::NoMemory)?;
                    if !out.is_empty() {
                        out.push_str(", ");
                    }
                    out.push_str(&name);
                }
                return Ok(out);
            }
        };
        self.messages.copy(host, key)
    }

    fn trigger(&mut self, host: &Host<'_>, trigger: &Trigger) -> Result<String, Error> {
        let setting = |w: &Self, id: &str| -> Result<String, Error> { w.setting(id) };
        match trigger {
            Trigger::Start => self.messages.copy(host, "rules.when.start"),
            Trigger::Time { minute, days } => {
                let time = self.clock(host, *minute)?;
                let days = self.days(host, *days)?;
                self.messages.format(
                    host,
                    "rules.when.time",
                    &[
                        Arg::Text {
                            name: "time",
                            value: &time,
                        },
                        Arg::Text {
                            name: "days",
                            value: &days,
                        },
                    ],
                )
            }
            Trigger::Every(m) => self.messages.format(
                host,
                "rules.when.every",
                &[
                    Arg::Count(i64::from(*m)),
                    Arg::Int {
                        name: "count",
                        value: i64::from(*m),
                    },
                ],
            ),
            Trigger::BookOpened => self.messages.copy(host, "rules.when.book"),
            Trigger::BookClosed => self.messages.copy(host, "rules.when.book-closed"),
            Trigger::BookFinished => self.messages.copy(host, "rules.when.book-finished"),
            Trigger::Resume => self.messages.copy(host, "rules.when.resume"),
            Trigger::Sleep => self.messages.copy(host, "rules.when.sleep"),
            Trigger::BatteryBelow(p) | Trigger::BatteryAtLeast(p) => {
                let key = if matches!(trigger, Trigger::BatteryBelow(_)) {
                    "rules.when.battery"
                } else {
                    "rules.when.battery-at"
                };
                self.messages.format(
                    host,
                    key,
                    &[Arg::Int {
                        name: "percent",
                        value: i64::from(*p),
                    }],
                )
            }
            Trigger::Charging => self.messages.copy(host, "rules.when.charging"),
            Trigger::Unplugged => self.messages.copy(host, "rules.when.unplugged"),
            Trigger::SwitchOn(id) | Trigger::SwitchOff(id) => {
                let title = setting(self, id)?;
                let key = if matches!(trigger, Trigger::SwitchOn(_)) {
                    "rules.when.switch-on"
                } else {
                    "rules.when.switch-off"
                };
                self.messages.format(
                    host,
                    key,
                    &[Arg::Text {
                        name: "setting",
                        value: &title,
                    }],
                )
            }
        }
    }

    fn condition(
        &mut self,
        host: &Host<'_>,
        condition: &Condition,
    ) -> Result<Option<String>, Error> {
        Ok(Some(match condition {
            Condition::Always => return Ok(None),
            Condition::Between { from, to } => {
                let (from, to) = (self.clock(host, *from)?, self.clock(host, *to)?);
                self.messages.format(
                    host,
                    "rules.if.between",
                    &[
                        Arg::Text {
                            name: "from",
                            value: &from,
                        },
                        Arg::Text {
                            name: "to",
                            value: &to,
                        },
                    ],
                )?
            }
            Condition::Reading => self.messages.copy(host, "rules.if.reading")?,
            Condition::NotReading => self.messages.copy(host, "rules.if.not-reading")?,
            Condition::OnBattery => self.messages.copy(host, "rules.if.battery")?,
            Condition::Charging => self.messages.copy(host, "rules.if.charging")?,
            Condition::SwitchOn(id) | Condition::SwitchOff(id) => {
                let title = self.setting(id)?;
                let key = if matches!(condition, Condition::SwitchOn(_)) {
                    "rules.if.switch-on"
                } else {
                    "rules.if.switch-off"
                };
                self.messages.format(
                    host,
                    key,
                    &[Arg::Text {
                        name: "setting",
                        value: &title,
                    }],
                )?
            }
        }))
    }

    /// When a rule applies, with its condition, in words.
    fn when(&mut self, host: &Host<'_>, rule: &Rule) -> Result<String, Error> {
        let when = self.trigger(host, &rule.trigger)?;
        match self.condition(host, &rule.condition)? {
            Some(condition) => self.messages.format(
                host,
                "rules.when-if",
                &[
                    Arg::Text {
                        name: "when",
                        value: &when,
                    },
                    Arg::Text {
                        name: "condition",
                        value: &condition,
                    },
                ],
            ),
            None => Ok(when),
        }
    }

    fn effect(&mut self, host: &Host<'_>, effect: &Effect) -> Result<String, Error> {
        match effect {
            Effect::Action(id) => copy(
                self.actions
                    .iter()
                    .find(|a| a.id == *id)
                    .map_or(id.as_str(), |a| a.title.as_str()),
            ),
            Effect::Switch(id, on) => {
                let title = self.setting(id)?;
                let state = self
                    .messages
                    .copy(host, if *on { "rules.on" } else { "rules.off" })?;
                self.messages.format(
                    host,
                    "rules.do.switch",
                    &[
                        Arg::Text {
                            name: "setting",
                            value: &title,
                        },
                        Arg::Text {
                            name: "state",
                            value: &state,
                        },
                    ],
                )
            }
            Effect::Number(id, value) => {
                let title = self.setting(id)?;
                self.messages.format(
                    host,
                    "rules.do.number",
                    &[
                        Arg::Text {
                            name: "setting",
                            value: &title,
                        },
                        Arg::Int {
                            name: "value",
                            value: *value,
                        },
                    ],
                )
            }
            Effect::Message(text) => {
                // The format arguments take 1,024 bytes: a longer message is
                // shown cut, not refused.
                let shown = shortened(text, MESSAGE_SHOWN)?;
                self.messages.format(
                    host,
                    "rules.do.message",
                    &[Arg::Text {
                        name: "text",
                        value: &shown,
                    }],
                )
            }
        }
    }
}

/// The most bytes of a message the list shows.
const MESSAGE_SHOWN: usize = 256;

/// `text` cut at a character boundary to at most `max` bytes, ending in "…"
/// where it was cut.
fn shortened(text: &str, max: usize) -> Result<String, Error> {
    if text.len() <= max {
        return copy(text);
    }
    let mut end = max.saturating_sub('…'.len_utf8());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    concat(&[&text[..end], "…"])
}

fn screen_token(screen: &str) -> Option<u32> {
    match screen {
        "rules.list" => Some(LIST),
        "rules.pick" => Some(PICK),
        _ => None,
    }
}

impl NativeRules {
    fn ui_event(&mut self, host: &Host<'_>, event: Event<'_>) -> Result<(), Error> {
        match event {
            Event::ScreenOpened {
                screen, instance, ..
            } => {
                self.ensure_ui(host)?;
                self.opened(host, screen, instance)
            }
            Event::ScreenShown {
                screen,
                instance: Some(instance),
                ..
            } => {
                if let Some(token) = screen_token(screen) {
                    self.screens.shown(token, instance)?;
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } => {
                if matches!(self.screens.get(instance), Some(View::Pick(_))) {
                    self.pick = None;
                }
                self.screens.close(instance);
                Ok(())
            }
            Event::Answered { answer, .. } => {
                self.ensure_ui(host)?;
                self.answered(host, answer)
            }
            _ => Ok(()),
        }
    }

    fn opened(&mut self, host: &Host<'_>, screen: &str, instance: Instance) -> Result<(), Error> {
        let view = match screen_token(screen) {
            Some(LIST) => {
                self.load_rules(host)?;
                if self.rules.is_none() {
                    let text = Messages::new()?.copy(host, "rules.unreadable")?;
                    self.notify(host, &text)?;
                }
                View::List(self.build_list(host, 0)?)
            }
            Some(PICK) => View::Pick(self.build_pick(host)?),
            _ => return Ok(()),
        };
        self.screens.open(instance, view)
    }

    /// The rows of the list: one a rule, then the one that adds a rule.
    fn build_list(&mut self, host: &Host<'_>, focus: usize) -> Result<ListView, Error> {
        let rules = self.rules.as_deref().unwrap_or_default();
        let mut words = Words::new(host, rules)?;
        let mut items = Vec::new();
        items
            .try_reserve_exact(rules.len().saturating_add(1))
            .map_err(|_| Error::NoMemory)?;
        for rule in rules {
            let title = words.when(host, rule)?;
            let note = words.effect(host, &rule.effect)?;
            let state = words
                .messages
                .copy(host, if rule.on { "rules.on" } else { "rules.off" })?;
            items.push(Item::noted(&title, &state, &note)?);
        }
        let add = words.messages.copy(host, "rules.add")?;
        items.push(Item::row(&add, "+")?);
        let mut list = ItemList::new(items);
        list.set_focus(focus);
        Ok(ListView {
            list,
            rules: rules.len(),
            empty: rules.is_empty(),
        })
    }

    /// Rebuilds the list on top of the stack, keeping the focus where it was.
    fn refresh_list(&mut self, host: &Host<'_>, focus: usize) -> Result<(), Error> {
        let view = match self.build_list(host, focus) {
            Ok(view) => view,
            Err(error) => {
                // The list shown would index the old rules: leave it rather
                // than let a tap edit the wrong rule.
                let mut scratch = [0; 64];
                let _ = CommandSender::new(host, &mut scratch).back();
                return Err(error);
            }
        };
        if let Some(slot) = self.screens.top(LIST) {
            *slot = View::List(view);
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch)
            .queue_redraw(RedrawMode::Partial)
            .or(Ok(()))
    }

    /// The choice of an action or a setting, as the draft needs it.
    fn build_pick(&mut self, host: &Host<'_>) -> Result<PickView, Error> {
        let Some(pick) = self.pick else {
            return Err(Error::State);
        };
        let mut reader = Reader::open(host)?;
        let found = (|| {
            Ok(match pick {
                PickFor::EffectAction => (reader.actions()?, "rules.pick.action"),
                PickFor::EffectNumber => (
                    Self::named(reader.settings()?, Sort::Number)?,
                    "rules.pick.number",
                ),
                PickFor::Trigger(_) | PickFor::EffectSwitch(_) | PickFor::Condition(..) => (
                    Self::named(reader.settings()?, Sort::Switch)?,
                    "rules.pick.switch",
                ),
            })
        })();
        let (mut named, key) = reader.finish(found)?;
        paperpaw_ui_kit::mem::stable_sort_by(&mut named, |a, b| {
            paperpaw_ui_kit::text::cmp_lowercase(&a.title, &b.title)
        });
        let title = Messages::new()?.copy(host, key)?;
        let mut items = Vec::new();
        let mut choices = Vec::new();
        items
            .try_reserve_exact(named.len())
            .map_err(|_| Error::NoMemory)?;
        choices
            .try_reserve_exact(named.len())
            .map_err(|_| Error::NoMemory)?;
        for entry in named {
            items.push(Item::row(&entry.title, "")?);
            choices.push(entry.id);
        }
        Ok(PickView {
            list: ItemList::new(items),
            title,
            choices,
        })
    }

    /// The settings of one sort, as named entries.
    fn named(settings: Vec<Setting>, sort: Sort) -> Result<Vec<Named>, Error> {
        let mut found = Vec::new();
        for s in settings.into_iter().filter(|s| s.sort == sort) {
            push(
                &mut found,
                Named {
                    id: s.id,
                    title: s.title,
                },
            )?;
        }
        Ok(found)
    }

    // ---- choosing and holding

    fn chosen(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        match token {
            LIST => {
                let Some(View::List(view)) = self.screens.top(LIST) else {
                    return Ok(());
                };
                if index < view.rules {
                    self.toggle(host, index)
                } else {
                    self.trigger = None;
                    self.ask_choice(
                        host,
                        "rules.when",
                        "rules.trigger.",
                        &TRIGGERS,
                        Purpose::When,
                    )
                }
            }
            PICK => self.picked(host, index),
            _ => Ok(()),
        }
    }

    fn held(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(View::List(view)) = self.screens.top(LIST) else {
            return Ok(());
        };
        if token != LIST || index >= view.rules {
            return self.chosen(host, token, index);
        }
        self.ask_choice(
            host,
            "rules.title",
            "rules.option.",
            &["condition", "remove"],
            Purpose::Option(index),
        )
    }

    /// Pauses a rule, or lets it run again.
    fn toggle(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        self.edit(host, index, |rules| {
            if let Some(rule) = rules.get_mut(index) {
                rule.on = !rule.on;
            }
            Ok(())
        })
    }

    /// Changes a copy of the rules, saves it and shows the list again.
    fn edit(
        &mut self,
        host: &Host<'_>,
        focus: usize,
        change: impl FnOnce(&mut Vec<Rule>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.load_rules(host)?;
        let Some(current) = self.rules.as_deref() else {
            let text = Messages::new()?.copy(host, "rules.unreadable")?;
            return self.notify(host, &text);
        };
        let mut rules = copy_rules(current)?;
        change(&mut rules)?;
        self.save(host, rules)?;
        self.load_rules(host)?;
        self.refresh_list(host, focus)
    }

    // ---- asking

    fn ask(
        &mut self,
        host: &Host<'_>,
        question: Question<'_>,
        purpose: Purpose,
    ) -> Result<(), Error> {
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self.ui()?.ask(host, question, &mut input, &mut output)?;
        self.asked = Some((request, purpose));
        Ok(())
    }

    /// A choice among words: `prefix` and each name make the catalog key.
    fn ask_choice(
        &mut self,
        host: &Host<'_>,
        title: &str,
        prefix: &str,
        names: &[&str],
        purpose: Purpose,
    ) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, title)?;
        let mut labels = String::new();
        for (at, name) in names.iter().enumerate() {
            let key = concat(&[prefix, name])?;
            let label = messages.copy(host, &key)?;
            labels
                .try_reserve(label.len() + 1)
                .map_err(|_| Error::NoMemory)?;
            if at > 0 {
                labels.push('\n');
            }
            labels.push_str(&label);
        }
        self.ask(host, Question::choice(&title, "", &labels)?, purpose)
    }

    fn ask_text(
        &mut self,
        host: &Host<'_>,
        title: &str,
        number: bool,
        purpose: Purpose,
    ) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, title)?;
        let mut question = Question::text(&title, "")?;
        if number {
            question = question.mode(TextMode::Number)?;
        }
        self.ask(host, question, purpose)
    }

    fn bad(&self, host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        self.notify(host, &text)
    }

    fn ask_what(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ask_choice(host, "rules.what", "rules.effect.", &EFFECTS, Purpose::What)
    }

    fn open_pick(&mut self, host: &Host<'_>, pick: PickFor) -> Result<(), Error> {
        self.pick = Some(pick);
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).open(Open::screen("rules.pick")?)
    }

    /// Leaves the choice of an action or a setting, once made.
    fn leave_pick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.pick.take().is_some() {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).back()?;
        }
        Ok(())
    }

    // ---- the answers

    #[expect(clippy::too_many_lines, reason = "one arm a step of the dialogs")]
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _)) = self.asked else {
            return Ok(());
        };
        if request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((_, purpose)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let choice = if answer.kind() == QuestionKind::Choice {
            usize::try_from(answer.choice()).unwrap_or(usize::MAX)
        } else {
            0
        };
        let typed = if answer.kind() == QuestionKind::Text {
            let text = uitext::read_all(self.ui()?, host, request.text(), ANSWER_LIMIT)?;
            copy(text.trim())?
        } else {
            String::new()
        };
        match purpose {
            Purpose::When => self.when_chosen(host, choice),
            Purpose::TimeText => match parse_time(&typed) {
                Some(minute) => self.ask_choice(
                    host,
                    "rules.days-title",
                    "rules.days.",
                    &["every", "weekdays", "weekends"],
                    Purpose::Days(minute),
                ),
                None => self.bad(host, "rules.bad-time"),
            },
            Purpose::Days(minute) => {
                let days = [EVERY_DAY, WEEKDAYS, WEEKENDS]
                    .get(choice)
                    .copied()
                    .unwrap_or(EVERY_DAY);
                self.trigger = Some(Trigger::Time { minute, days });
                self.ask_what(host)
            }
            Purpose::EveryText => match typed.parse::<u16>() {
                Ok(m) if m > 0 => {
                    self.trigger = Some(Trigger::Every(m));
                    self.ask_what(host)
                }
                _ => self.bad(host, "rules.bad-number"),
            },
            Purpose::BatteryText => match parse_percent(&typed) {
                Some(p) => {
                    self.trigger = Some(Trigger::BatteryBelow(p));
                    self.ask_what(host)
                }
                None => self.bad(host, "rules.bad-number"),
            },
            Purpose::Level => match [80u8, 90, 100].get(choice) {
                Some(p) => {
                    self.trigger = Some(Trigger::BatteryAtLeast(*p));
                    self.ask_what(host)
                }
                None => self.ask_text(host, "rules.level-other-title", true, Purpose::LevelText),
            },
            Purpose::LevelText => match parse_percent(&typed) {
                Some(p) => {
                    self.trigger = Some(Trigger::BatteryAtLeast(p));
                    self.ask_what(host)
                }
                None => self.bad(host, "rules.bad-number"),
            },
            Purpose::What => match EFFECTS.get(choice).copied() {
                Some("action") => self.open_pick(host, PickFor::EffectAction),
                Some("switch-on") => self.open_pick(host, PickFor::EffectSwitch(true)),
                Some("switch-off") => self.open_pick(host, PickFor::EffectSwitch(false)),
                Some("number") => self.open_pick(host, PickFor::EffectNumber),
                Some("message") => {
                    self.ask_text(host, "rules.message-title", false, Purpose::MessageText)
                }
                _ => Ok(()),
            },
            Purpose::MessageText if typed.is_empty() => Ok(()),
            Purpose::MessageText => self.finish(host, Effect::Message(typed)),
            Purpose::NumberText(id, min, max) => match typed.parse::<i64>() {
                Ok(v) if (min..=max).contains(&v) => {
                    self.leave_pick(host)?;
                    self.finish(host, Effect::Number(id, v))
                }
                _ => self.bad(host, "rules.bad-number"),
            },
            Purpose::Option(index) => match choice {
                0 => self.ask_choice(
                    host,
                    "rules.only-if",
                    "rules.condition.",
                    &CONDITIONS,
                    Purpose::Condition(index),
                ),
                _ => self.edit(host, index, |rules| {
                    if index < rules.len() {
                        rules.remove(index);
                    }
                    Ok(())
                }),
            },
            Purpose::Condition(index) => match CONDITIONS.get(choice).copied() {
                Some("between") => {
                    self.ask_text(host, "rules.from-title", false, Purpose::FromText(index))
                }
                Some("switch-on") => self.open_pick(host, PickFor::Condition(index, true)),
                Some("switch-off") => self.open_pick(host, PickFor::Condition(index, false)),
                Some("reading") => self.set_condition(host, index, Condition::Reading),
                Some("not-reading") => self.set_condition(host, index, Condition::NotReading),
                Some("battery") => self.set_condition(host, index, Condition::OnBattery),
                Some("charging") => self.set_condition(host, index, Condition::Charging),
                _ => self.set_condition(host, index, Condition::Always),
            },
            Purpose::FromText(index) => match parse_time(&typed) {
                Some(from) => {
                    self.ask_text(host, "rules.to-title", false, Purpose::ToText(index, from))
                }
                None => self.bad(host, "rules.bad-time"),
            },
            Purpose::ToText(index, from) => match parse_time(&typed) {
                Some(to) => self.set_condition(host, index, Condition::Between { from, to }),
                None => self.bad(host, "rules.bad-time"),
            },
        }
    }

    fn when_chosen(&mut self, host: &Host<'_>, choice: usize) -> Result<(), Error> {
        let plain = match TRIGGERS.get(choice).copied() {
            Some("start") => Trigger::Start,
            Some("book") => Trigger::BookOpened,
            Some("book-closed") => Trigger::BookClosed,
            Some("book-finished") => Trigger::BookFinished,
            Some("resume") => Trigger::Resume,
            Some("sleep") => Trigger::Sleep,
            Some("charging") => Trigger::Charging,
            Some("unplugged") => Trigger::Unplugged,
            Some("time") => {
                return self.ask_text(host, "rules.time-title", false, Purpose::TimeText);
            }
            Some("every") => {
                return self.ask_text(host, "rules.every-title", true, Purpose::EveryText);
            }
            Some("battery") => {
                return self.ask_text(host, "rules.battery-title", true, Purpose::BatteryText);
            }
            Some("battery-at") => {
                return self.ask_choice(
                    host,
                    "rules.level-title",
                    "rules.level.",
                    &["80", "90", "100", "other"],
                    Purpose::Level,
                );
            }
            Some("switch-on") => return self.open_pick(host, PickFor::Trigger(true)),
            Some("switch-off") => return self.open_pick(host, PickFor::Trigger(false)),
            _ => return Ok(()),
        };
        self.trigger = Some(plain);
        self.ask_what(host)
    }

    /// Adds the rule drafted, doing `effect`.
    fn finish(&mut self, host: &Host<'_>, effect: Effect) -> Result<(), Error> {
        let Some(trigger) = self.trigger.take() else {
            return Ok(());
        };
        self.leave_pick(host)?;
        let focus = self.rules.as_deref().map_or(0, <[Rule]>::len);
        self.edit(host, focus, |rules| {
            push(
                rules,
                Rule {
                    trigger,
                    effect,
                    on: true,
                    condition: Condition::Always,
                },
            )
        })
    }

    fn set_condition(
        &mut self,
        host: &Host<'_>,
        index: usize,
        condition: Condition,
    ) -> Result<(), Error> {
        self.leave_pick(host)?;
        self.edit(host, index, |rules| {
            if let Some(rule) = rules.get_mut(index) {
                rule.condition = condition;
            }
            Ok(())
        })
    }

    /// An entry of the choice of an action or a setting was taken.
    fn picked(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let (Some(pick), Some(View::Pick(view))) = (self.pick, self.screens.top(PICK)) else {
            return Ok(());
        };
        let Some(id) = view.choices.get(index) else {
            return Ok(());
        };
        let id = copy(id)?;
        match pick {
            PickFor::Trigger(on) => {
                self.trigger = Some(if on {
                    Trigger::SwitchOn(id)
                } else {
                    Trigger::SwitchOff(id)
                });
                self.leave_pick(host)?;
                self.ask_what(host)
            }
            PickFor::EffectAction => self.finish(host, Effect::Action(id)),
            PickFor::EffectSwitch(on) => self.finish(host, Effect::Switch(id, on)),
            PickFor::Condition(rule, on) => self.set_condition(
                host,
                rule,
                if on {
                    Condition::SwitchOn(id)
                } else {
                    Condition::SwitchOff(id)
                },
            ),
            PickFor::EffectNumber => {
                let mut reader = Reader::open(host)?;
                let bounds = reader.bounds(&id);
                let (min, max) = reader.finish(bounds)?;
                let title = Messages::new()?.format(
                    host,
                    "rules.number-title",
                    &[
                        Arg::Int {
                            name: "min",
                            value: min,
                        },
                        Arg::Int {
                            name: "max",
                            value: max,
                        },
                    ],
                )?;
                let question = Question::text(&title, "")?.mode(TextMode::Number)?;
                self.ask(host, question, Purpose::NumberText(id, min, max))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MESSAGE_SHOWN, chosen_or_system, shortened};

    #[test]
    fn with_no_zone_chosen_the_systems_offset_counts() {
        // "automatic" (the default) on a system two hours ahead: 08:41 UTC
        // is 10:41 for the rules, as in Beta 3 (#257).
        assert_eq!(chosen_or_system(None, Some(120)), 7_200);
        assert_eq!(chosen_or_system(None, Some(-330)), -19_800);
        // A chosen zone wins over the system's; no clock zone at all is UTC.
        assert_eq!(chosen_or_system(Some(3_600), Some(120)), 3_600);
        assert_eq!(chosen_or_system(Some(0), Some(120)), 0);
        assert_eq!(chosen_or_system(None, None), 0);
    }

    #[test]
    fn a_long_message_is_shown_cut_at_a_character() {
        assert_eq!(shortened("short", MESSAGE_SHOWN).unwrap(), "short");
        let long = "é".repeat(2000);
        let cut = shortened(&long, MESSAGE_SHOWN).unwrap();
        assert!(cut.len() <= MESSAGE_SHOWN && cut.ends_with('…'));
        assert!(cut.chars().all(|c| c == 'é' || c == '…'));
    }
}
