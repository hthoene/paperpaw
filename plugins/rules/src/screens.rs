//! The list of rules, and the list to pick an action or a switch from.
//!
//! Adding a rule asks, one after the other: when (a choice; for a time of
//! day the time on the keyboard and the days, for a repeat the minutes,
//! for the battery falling low the percentage, for it reaching a level
//! which level (a choice, or another on the keyboard), for a switch which
//! one), what (a choice: run an action, turn a switch on or off, set a
//! number, show a message), then which action, switch or number
//! ([`PickScreen`]) and its value, or the message. A rule's options add a
//! condition (only if …): between two times, while reading or not, on
//! battery or while charging, while a switch is on or off.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::builtin::settings::SCOPE_ONLY;
use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::ui::choice;
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, SettingKind,
};

use crate::rule::{self, Condition, EVERY_DAY, Effect, Rule, Trigger, WEEKDAYS, WEEKENDS};
use crate::{PICK_SCREEN, Shared, effect_text, trigger_text, when_text};

/// Replies: a trigger, a time, an effect, an action or switch picked, a
/// rule's option.
const TRIGGER_CHOSEN: Event<choice::Picked> = Event::new("rules.trigger-chosen");
const TIME_ENTERED: Event<Typed> = Event::new("rules.time-entered");
const DAYS_CHOSEN: Event<choice::Picked> = Event::new("rules.days-chosen");
const EVERY_ENTERED: Event<Typed> = Event::new("rules.every-entered");
const BATTERY_ENTERED: Event<Typed> = Event::new("rules.battery-entered");
/// The level a battery reaches: chosen, or another one entered.
const LEVEL_CHOSEN: Event<choice::Picked> = Event::new("rules.level-chosen");
const LEVEL_ENTERED: Event<Typed> = Event::new("rules.level-entered");
/// The levels offered, then another one.
const LEVELS: [&str; 4] = ["80", "90", "100", "other"];
/// The switch a trigger watches was picked.
const TRIGGER_PICKED: Id = Id::new_static("rules.trigger-picked");
/// The triggers offered, in this order.
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
/// The days a time of day may apply: every day, weekdays, weekends, or
/// one day (Monday first).
const DAYS: [&str; 10] = [
    "every", "weekdays", "weekends", "1", "2", "3", "4", "5", "6", "0",
];
const EFFECT_CHOSEN: Event<choice::Picked> = Event::new("rules.effect-chosen");
/// What a rule may do, in this order.
const EFFECTS: [&str; 5] = ["action", "switch-on", "switch-off", "number", "message"];
const PICKED: Id = Id::new_static("rules.picked");
const NUMBER_ENTERED: Event<Typed> = Event::new("rules.number-entered");
const MESSAGE_ENTERED: Event<Typed> = Event::new("rules.message-entered");
const OPTION_CHOSEN: Event<choice::Picked> = Event::new("rules.option-chosen");
/// A rule's condition was chosen; for times between, the first and the
/// second time entered; for a switch, which one.
const CONDITION_CHOSEN: Event<choice::Picked> = Event::new("rules.condition-chosen");
const FROM_ENTERED: Event<Typed> = Event::new("rules.from-entered");
const TO_ENTERED: Event<Typed> = Event::new("rules.to-entered");
const CONDITION_PICKED: Id = Id::new_static("rules.condition-picked");
/// The conditions offered, in this order.
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
/// Picking an action, a switch or a number.
const ACTION: &str = "action";
const SWITCH: &str = "switch";
const NUMBER: &str = "number";

/// A rule being added: when, then what kind of effect.
#[derive(Debug, Clone, Default)]
struct Draft {
    trigger: Option<Trigger>,
    /// The time of day entered, until its days are chosen.
    minute: Option<u16>,
    /// The switch to watch is turned on (`true`) or off.
    watch_on: bool,
    /// `action`, `switch-on`, `switch-off` or `number`.
    effect: Option<&'static str>,
    /// The number picked, until its value is entered.
    number: Option<Id>,
    /// The rule whose condition is being chosen.
    editing: Option<usize>,
    /// The first time of a condition between two, until the second.
    from: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Rule(usize),
    Add,
}

pub(crate) struct RulesScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    /// The line at the bottom.
    status: Rect,
    state: Shared,
    rules: Vec<Rule>,
    /// Whether the rules could be read: if not, nothing is saved over
    /// them.
    readable: bool,
    draft: Draft,
    message: Option<String>,
}

impl RulesScreen {
    pub(crate) fn new(env: &Env<'_>, state: Shared) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let line = (style.row()) * 3 / 2;
        let status = Rect::new(0, display.height - line, display.width, line);
        let area = Rect::new(
            0,
            top,
            display.width,
            display.height.saturating_sub(top + line),
        );
        let mut screen = Self {
            list: ListView::new(area, style.noted_row()).focus_on_keys(env),
            status,
            state,
            rules: Vec::new(),
            readable: true,
            draft: Draft::default(),
            message: None,
            style,
            header,
        };
        screen.reload(env);
        screen
    }

    /// The rules as kept; where they cannot be read, none, and a word why.
    fn reload(&mut self, env: &Env<'_>) {
        let loaded = self.state.store.get(env.storage());
        self.readable = loaded.is_some();
        self.rules = loaded.unwrap_or_default();
        if !self.readable {
            self.message = Some(env.i18n().tr("rules.unreadable").to_string());
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = (0..self.rules.len()).map(Row::Rule).collect();
        rows.push(Row::Add);
        rows
    }

    fn redraw(&self, cx: &mut Cx<'_, '_>) {
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Fast);
    }

    /// Keeps the rules and sets the timers again. Not kept (the card
    /// could not be written, or the rules could not be read), the list
    /// goes back to what the card has, and says why.
    fn save(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let saved = self.readable
            && self
                .state
                .store
                .save(env.storage(), self.rules.clone())
                .is_ok();
        if !saved {
            self.reload(&env);
            if self.readable {
                self.message = Some(env.i18n().tr("rules.not-saved").to_string());
            }
        }
        let mut out = paperpaw_sdk::Commands::new();
        crate::schedule(&env, &self.state.store, &mut out);
        for command in out {
            cx.command(command);
        }
    }

    fn activate(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        match self.rows().get(index).copied() {
            Some(Row::Rule(i)) => {
                self.rules[i].on = !self.rules[i].on;
                self.save(cx);
                self.redraw(cx);
            }
            Some(Row::Add) => {
                self.draft = Draft::default();
                cx.command(
                    paperpaw_sdk::ask::choice(env.i18n().tr("rules.when"), &TRIGGER_CHOSEN)
                        .options("rules.trigger.", &TRIGGERS)
                        .command(),
                );
            }
            None => {}
        }
    }

    fn offer(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let Some(Row::Rule(i)) = self.rows().get(index).copied() else {
            return self.activate(index, cx);
        };
        let title = trigger_text(&env, &self.rules[i].trigger);
        cx.command(
            paperpaw_sdk::ask::choice(&title, &OPTION_CHOSEN)
                .about(i.to_string())
                .options("rules.option.", &["condition", "remove"])
                .command(),
        );
    }

    /// Adds the rule drafted, doing `effect`.
    fn finish(&mut self, effect: Effect, cx: &mut Cx<'_, '_>) {
        let Some(trigger) = self.draft.trigger.clone() else {
            return;
        };
        self.draft = Draft::default();
        self.rules.push(Rule {
            trigger,
            effect,
            on: true,
            condition: Condition::Always,
        });
        self.save(cx);
        self.list.set_focus(self.rules.len().saturating_sub(1));
        self.redraw(cx);
    }

    /// Gives the rule being edited `condition`.
    fn set_condition(&mut self, condition: Condition, cx: &mut Cx<'_, '_>) {
        let Some(i) = self.draft.editing.take().filter(|&i| i < self.rules.len()) else {
            return;
        };
        self.rules[i].condition = condition;
        self.save(cx);
        self.redraw(cx);
    }

    /// Asks what the new rule does.
    fn ask_effect(&self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        cx.command(
            paperpaw_sdk::ask::choice(env.i18n().tr("rules.what"), &EFFECT_CHOSEN)
                .options("rules.effect.", &EFFECTS)
                .command(),
        );
    }

    fn move_focus(&mut self, delta: isize, cx: &mut Cx<'_, '_>) {
        let len = self.rows().len();
        self.list.move_focus(delta, len).request(cx);
    }
}

impl Screen for RulesScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let rows = self.rows();
        self.header.draw(
            &mut ui,
            env.i18n().tr("rules.title"),
            None,
            self.list.paging(rows.len()),
        );
        let rules = &self.rules;
        self.list
            .draw(&mut ui, rows.len(), |ui, index, rect, focused| {
                match rows[index] {
                    Row::Rule(i) => {
                        let rule = &rules[i];
                        let title = when_text(env, rule);
                        let note = effect_text(env, &rule.effect);
                        ui::Row::switch(&title, rule.on)
                            .note(&note)
                            .focused(focused)
                            .draw(ui, rect);
                    }
                    Row::Add => {
                        let title = env.i18n().tr("rules.add");
                        ui::Row::value(title, "+").focused(focused).draw(ui, rect);
                    }
                }
            });
        if self.rules.is_empty() {
            // Below the row that adds one: what rules are for.
            let top = self.list.area().y + self.list.row() as u16;
            let area = Rect::new(
                0,
                top,
                self.list.area().width,
                self.status.y.saturating_sub(top),
            );
            ui.empty(
                area,
                "automation",
                env.i18n().tr("rules.empty.title"),
                env.i18n().tr("rules.empty"),
            );
            if self.message.is_none() {
                return;
            }
        }
        ui.canvas().fill(self.status, Color::WHITE);
        let text = self
            .message
            .clone()
            .unwrap_or_else(|| ui::tr_input(env, "rules.hint").to_string());
        ui.note_line(self.status, &text);
    }

    fn on_show(&mut self, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        self.reload(&env);
        self.redraw(cx);
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if let Some(delta) = ui::list_step(a, self.list.rows() as isize) {
            self.move_focus(delta, cx);
        } else if *a == actions::CONFIRM {
            self.activate(self.list.focus(), cx);
        } else if *a == actions::MENU {
            self.offer(self.list.focus(), cx);
        } else if *a == actions::BACK {
            cx.command(Command::Back);
        } else {
            return Handled::No;
        }
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        let len = self.rows().len();
        let paging = self.list.paging(len);
        if gesture.kind == GestureKind::Tap
            && let Some(action) = self
                .header
                .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        let Some(index) = self.list.hit(gesture.start, len) else {
            return Handled::No;
        };
        self.list.set_focus(index);
        self.message = None;
        match gesture.kind {
            GestureKind::Tap => self.activate(index, cx),
            GestureKind::LongPress => self.offer(index, cx),
            _ => return Handled::No,
        }
        Handled::Yes
    }

    fn on_event(&mut self, event: &Id, params: &Params, cx: &mut Cx<'_, '_>) {
        let answer = Answer::read(event, params);
        if *event == TRIGGER_CHOSEN
            || *event == TIME_ENTERED
            || *event == DAYS_CHOSEN
            || *event == EVERY_ENTERED
            || *event == BATTERY_ENTERED
            || *event == LEVEL_CHOSEN
            || *event == LEVEL_ENTERED
            || *event == TRIGGER_PICKED
        {
            self.on_trigger(&answer, cx);
        } else if *event == EFFECT_CHOSEN
            || *event == PICKED
            || *event == NUMBER_ENTERED
            || *event == MESSAGE_ENTERED
        {
            self.on_effect(&answer, cx);
        } else {
            self.on_option(&answer, cx);
        }
    }
}

/// The answers to what the screen asked, by what they are about.
impl RulesScreen {
    /// When a new rule applies: chosen, a time and its days, a number,
    /// a switch.
    fn on_trigger(&mut self, answer: &Answer<'_>, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let (event, params) = (answer.event, answer.params);
        let (option, text) = (answer.option(), answer.text());
        if *event == TRIGGER_CHOSEN {
            self.draft.trigger = match option {
                "start" => Some(Trigger::Start),
                "book" => Some(Trigger::BookOpened),
                "book-closed" => Some(Trigger::BookClosed),
                "book-finished" => Some(Trigger::BookFinished),
                "resume" => Some(Trigger::Resume),
                "sleep" => Some(Trigger::Sleep),
                "charging" => Some(Trigger::Charging),
                "unplugged" => Some(Trigger::Unplugged),
                "time" => return ask_text(cx, "rules.time-title", "22:00", &TIME_ENTERED),
                "every" => return ask_text(cx, "rules.every-title", "30", &EVERY_ENTERED),
                "battery" => return ask_text(cx, "rules.battery-title", "20", &BATTERY_ENTERED),
                // Which level, with what to know about it.
                "battery-at" => {
                    let title = env.i18n().tr("rules.level-title");
                    let ask = paperpaw_sdk::ask::choice(title, &LEVEL_CHOSEN);
                    cx.command(ask.options("rules.level.", &LEVELS).command());
                    return;
                }
                "switch-on" | "switch-off" => {
                    self.draft.watch_on = option == "switch-on";
                    cx.command(Command::OpenScreen(
                        PICK_SCREEN,
                        Params::new()
                            .with(params::MODE, SWITCH)
                            .with(params::REPLY, TRIGGER_PICKED.as_str()),
                    ));
                    return;
                }
                _ => return,
            };
            self.ask_effect(cx);
        } else if *event == TIME_ENTERED {
            match rule::parse_time(text) {
                Some(minute) => {
                    self.draft.minute = Some(minute);
                    cx.command(
                        paperpaw_sdk::ask::choice(env.i18n().tr("rules.days-title"), &DAYS_CHOSEN)
                            .options("rules.days.", &DAYS)
                            .command(),
                    );
                }
                None => cx.command(Command::Notify(env.i18n().tr("rules.bad-time").to_string())),
            }
        } else if *event == DAYS_CHOSEN {
            let days = match option {
                "every" => EVERY_DAY,
                "weekdays" => WEEKDAYS,
                "weekends" => WEEKENDS,
                day => match day.parse::<u8>() {
                    Ok(d) if d < 7 => 1 << d,
                    _ => return,
                },
            };
            if let Some(minute) = self.draft.minute.take() {
                self.draft.trigger = Some(Trigger::Time { minute, days });
                self.ask_effect(cx);
            }
        } else if *event == LEVEL_CHOSEN && option == "other" {
            ask_text(cx, "rules.level-other-title", "80", &LEVEL_ENTERED);
        } else if *event == EVERY_ENTERED
            || *event == BATTERY_ENTERED
            || *event == LEVEL_CHOSEN
            || *event == LEVEL_ENTERED
        {
            let trigger = if *event == EVERY_ENTERED {
                text.trim()
                    .parse::<u16>()
                    .ok()
                    .filter(|m| (1..=24 * 60).contains(m))
                    .map(Trigger::Every)
            } else if *event == BATTERY_ENTERED {
                rule::parse_percent(text).map(Trigger::BatteryBelow)
            } else {
                let level = if *event == LEVEL_CHOSEN { option } else { text };
                rule::parse_percent(level).map(Trigger::BatteryAtLeast)
            };
            let Some(trigger) = trigger else {
                let text = env.i18n().tr("rules.bad-number").to_string();
                cx.command(Command::Notify(text));
                return;
            };
            self.draft.trigger = Some(trigger);
            self.ask_effect(cx);
        } else if *event == TRIGGER_PICKED {
            let Some(id) = params.get(params::ID).and_then(|id| Id::parse(id).ok()) else {
                return;
            };
            self.draft.trigger = Some(if self.draft.watch_on {
                Trigger::SwitchOn(id)
            } else {
                Trigger::SwitchOff(id)
            });
            self.ask_effect(cx);
        }
    }

    /// What a new rule does: chosen, then what it acts on, a number, a
    /// message.
    fn on_effect(&mut self, answer: &Answer<'_>, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let (event, params) = (answer.event, answer.params);
        let (option, text) = (answer.option(), answer.text());
        if *event == EFFECT_CHOSEN {
            let (effect, kind) = match option {
                "action" => ("action", ACTION),
                "switch-on" => ("switch-on", SWITCH),
                "switch-off" => ("switch-off", SWITCH),
                "number" => ("number", NUMBER),
                "message" => return ask_text(cx, "rules.message-title", "", &MESSAGE_ENTERED),
                _ => return,
            };
            self.draft.effect = Some(effect);
            cx.command(Command::OpenScreen(
                PICK_SCREEN,
                Params::new()
                    .with(params::MODE, kind)
                    .with(params::REPLY, PICKED.as_str()),
            ));
        } else if *event == PICKED {
            let (Some(kind), Some(id)) = (
                self.draft.effect,
                params.get(params::ID).and_then(|id| Id::parse(id).ok()),
            ) else {
                return;
            };
            let effect = match kind {
                "action" => Effect::Action(id),
                "switch-on" => Effect::Switch(id, true),
                "switch-off" => Effect::Switch(id, false),
                _ => {
                    // A number: which value?
                    let Some((min, max)) = range(&env, &id) else {
                        return;
                    };
                    let title = env.i18n().format(
                        "rules.number-title",
                        &[("min", Arg::Int(min)), ("max", Arg::Int(max))],
                    );
                    let now = env.settings().int(id.as_str()).to_string();
                    self.draft.number = Some(id);
                    cx.command(ask::text(title, &NUMBER_ENTERED).initial(now).command());
                    return;
                }
            };
            self.finish(effect, cx);
        } else if *event == NUMBER_ENTERED {
            let Some(id) = self.draft.number.clone() else {
                return;
            };
            let value = text.trim().trim_end_matches('%').trim().parse::<i64>().ok();
            match (value, range(&env, &id)) {
                (Some(v), Some((min, max))) if (min..=max).contains(&v) => {
                    self.finish(Effect::Number(id, v), cx);
                }
                _ => {
                    let text = env.i18n().tr("rules.bad-number").to_string();
                    cx.command(Command::Notify(text));
                }
            }
        } else if *event == MESSAGE_ENTERED {
            let text = text.trim();
            if !text.is_empty() {
                self.finish(Effect::Message(text.to_string()), cx);
            }
        }
    }

    /// A rule's options: its condition (chosen, its times, its switch),
    /// or removing it.
    fn on_option(&mut self, answer: &Answer<'_>, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let (event, params) = (answer.event, answer.params);
        let (option, text) = (answer.option(), answer.text());
        let at = answer.at();
        if *event == OPTION_CHOSEN && option == "condition" {
            if let Some(i) = at.filter(|&i| i < self.rules.len()) {
                self.draft = Draft {
                    editing: Some(i),
                    ..Draft::default()
                };
                cx.command(
                    paperpaw_sdk::ask::choice(env.i18n().tr("rules.only-if"), &CONDITION_CHOSEN)
                        .options("rules.condition.", &CONDITIONS)
                        .command(),
                );
            }
        } else if *event == CONDITION_CHOSEN {
            let condition = match option {
                "always" => Condition::Always,
                "reading" => Condition::Reading,
                "not-reading" => Condition::NotReading,
                "battery" => Condition::OnBattery,
                "charging" => Condition::Charging,
                "between" => return ask_text(cx, "rules.from-title", "22:00", &FROM_ENTERED),
                "switch-on" | "switch-off" => {
                    self.draft.watch_on = option == "switch-on";
                    cx.command(Command::OpenScreen(
                        PICK_SCREEN,
                        Params::new()
                            .with(params::MODE, SWITCH)
                            .with(params::REPLY, CONDITION_PICKED.as_str()),
                    ));
                    return;
                }
                _ => return,
            };
            self.set_condition(condition, cx);
        } else if *event == FROM_ENTERED || *event == TO_ENTERED {
            let Some(minute) = rule::parse_time(text) else {
                cx.command(Command::Notify(env.i18n().tr("rules.bad-time").to_string()));
                return;
            };
            if *event == FROM_ENTERED {
                self.draft.from = Some(minute);
                return ask_text(cx, "rules.to-title", "06:00", &TO_ENTERED);
            }
            if let Some(from) = self.draft.from.take() {
                self.set_condition(Condition::Between { from, to: minute }, cx);
            }
        } else if *event == CONDITION_PICKED {
            let Some(id) = params.get(params::ID).and_then(|id| Id::parse(id).ok()) else {
                return;
            };
            let condition = if self.draft.watch_on {
                Condition::SwitchOn(id)
            } else {
                Condition::SwitchOff(id)
            };
            self.set_condition(condition, cx);
        } else if *event == OPTION_CHOSEN
            && option == "remove"
            && let Some(i) = at.filter(|&i| i < self.rules.len())
        {
            self.rules.remove(i);
            self.save(cx);
            self.list.set_len(self.rows().len());
            self.redraw(cx);
        }
    }
}

/// An answer to what the screen asked: the option chosen (and the rule
/// it was chosen for) or the text typed.
struct Answer<'a> {
    event: &'a Id,
    params: &'a Params,
    picked: Option<choice::Picked>,
    typed: Option<Typed>,
}

impl<'a> Answer<'a> {
    fn read(event: &'a Id, params: &'a Params) -> Self {
        let picked = [
            &TRIGGER_CHOSEN,
            &DAYS_CHOSEN,
            &LEVEL_CHOSEN,
            &EFFECT_CHOSEN,
            &OPTION_CHOSEN,
            &CONDITION_CHOSEN,
        ]
        .into_iter()
        .find_map(|e| e.read(event, params));
        let typed = [
            &TIME_ENTERED,
            &EVERY_ENTERED,
            &BATTERY_ENTERED,
            &LEVEL_ENTERED,
            &NUMBER_ENTERED,
            &MESSAGE_ENTERED,
            &FROM_ENTERED,
            &TO_ENTERED,
        ]
        .into_iter()
        .find_map(|e| e.read(event, params));
        Self {
            event,
            params,
            picked,
            typed,
        }
    }

    fn option(&self) -> &str {
        self.picked.as_ref().map_or("", |p| p.option.as_str())
    }

    /// The rule an option was chosen for (its index).
    fn at(&self) -> Option<usize> {
        self.picked.as_ref().and_then(|p| p.path.parse().ok())
    }

    fn text(&self) -> &str {
        self.typed.as_ref().map_or("", |t| t.text.as_str())
    }
}

/// Asks for a text, `initial` offered; it comes back as `reply`.
fn ask_text(cx: &mut Cx<'_, '_>, title: &str, initial: &str, reply: &Event<Typed>) {
    let env = *cx.env();
    cx.command(
        ask::text(env.i18n().tr(title), reply)
            .initial(initial)
            .command(),
    );
}

/// The range of number setting `id`.
fn range(env: &Env<'_>, id: &Id) -> Option<(i64, i64)> {
    env.settings().specs().find_map(|s| match s.kind {
        SettingKind::Int { min, max } if s.id == *id => Some((min, max)),
        _ => None,
    })
}

/// What can be picked that one owner (a plugin: the first segment of the
/// ids) brings.
struct Group {
    name: String,
    /// Ids and names, in alphabetical order.
    entries: Vec<(Id, String)>,
}

/// `entries` by their names, and by their owners where there are more
/// than fit a page (`rows`) and more than one owner: a group for each, by
/// the name `owner` gives it, in alphabetical order. Else one group with no
/// name, and `false`.
fn arrange(
    mut entries: Vec<(Id, String)>,
    rows: usize,
    owner: impl Fn(&str) -> String,
) -> (Vec<Group>, bool) {
    entries.sort_by_key(|e| e.1.to_lowercase());
    entries.dedup_by(|a, b| a.0 == b.0);
    let mut owners: Vec<&str> = Vec::new();
    for (id, _) in &entries {
        if !owners.contains(&id.namespace()) {
            owners.push(id.namespace());
        }
    }
    if entries.len() <= rows || owners.len() < 2 {
        let all = Group {
            name: String::new(),
            entries,
        };
        return (alloc::vec![all], false);
    }
    let mut by_owner: Vec<(String, Group)> = Vec::new();
    for entry in entries {
        let namespace = entry.0.namespace();
        match by_owner.iter_mut().find(|(n, _)| n == namespace) {
            Some((_, group)) => group.entries.push(entry),
            None => {
                let name = owner(namespace);
                let namespace = String::from(namespace);
                by_owner.push((
                    namespace,
                    Group {
                        name,
                        entries: alloc::vec![entry],
                    },
                ));
            }
        }
    }
    let mut groups: Vec<Group> = by_owner.into_iter().map(|(_, group)| group).collect();
    groups.sort_by_key(|g| g.name.to_lowercase());
    (groups, true)
}

/// Picks an action (parameter `mode` = `action`), a switch (`switch`) or a
/// number (`number`) by its name, in alphabetical order. Where the list is
/// longer than a page, the plugins that bring them come first, and a
/// plugin's own are one step in (back goes out again). The choice comes
/// back as the event named by `reply`, with `id`.
pub(crate) struct PickScreen {
    style: UiStyle,
    header: ui::Header,
    list: ListView,
    title: String,
    /// What there is to pick: one group where it is not shown by owner.
    groups: Vec<Group>,
    /// Whether the groups are listed before their entries.
    grouped: bool,
    /// The group whose entries are listed; none while the groups are.
    open: Option<usize>,
    reply: Option<Id>,
}

impl PickScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let header = ui::Header::new(env, &style, true);
        let display = env.device().display;
        let top = header.area().height + style.gap();
        let area = Rect::new(0, top, display.width, display.height.saturating_sub(top));
        let mode = params.get(params::MODE).unwrap_or(ACTION);
        let entries: Vec<(Id, String)> = if mode == SWITCH || mode == NUMBER {
            env.settings()
                .specs()
                // "Only this book" means nothing outside a book.
                .filter(|s| match s.kind {
                    SettingKind::Bool => mode == SWITCH && s.id != SCOPE_ONLY,
                    SettingKind::Int { .. } => mode == NUMBER,
                    _ => false,
                })
                .map(|s| (s.id.clone(), env.i18n().tr(&s.title).to_string()))
                .collect()
        } else {
            env.actions()
                .iter()
                .map(|a| (a.id().clone(), env.i18n().tr(a.title()).to_string()))
                .collect()
        };
        let list = ListView::new(area, u32::from(style.row())).focus_on_keys(env);
        let (groups, grouped) = arrange(entries, list.rows(), |namespace| {
            env.plugins().get(namespace).map_or_else(
                || namespace.to_string(),
                |plugin| plugin.name(env.i18n()).to_string(),
            )
        });
        let title = env
            .i18n()
            .tr(match mode {
                SWITCH => "rules.pick.switch",
                NUMBER => "rules.pick.number",
                _ => "rules.pick.action",
            })
            .to_string();
        Self {
            list,
            title,
            open: (!grouped).then_some(0),
            groups,
            grouped,
            reply: params.get(params::REPLY).and_then(|r| Id::parse(r).ok()),
            style,
            header,
        }
    }

    /// How many rows the list has now: the groups, or the open one's entries.
    fn len(&self) -> usize {
        match self.open {
            Some(group) => self.groups.get(group).map_or(0, |g| g.entries.len()),
            None => self.groups.len(),
        }
    }

    /// What the header says: the group open, else what is picked.
    fn heading(&self) -> &str {
        match self.open.and_then(|group| self.groups.get(group)) {
            Some(group) if self.grouped => &group.name,
            _ => &self.title,
        }
    }

    /// Row `index`: a group opens, an entry is picked.
    fn pick(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some(group) = self.open else {
            if index < self.groups.len() {
                self.open = Some(index);
                self.show_list(0, cx);
            }
            return;
        };
        let (Some((id, _)), Some(reply)) = (
            self.groups.get(group).and_then(|g| g.entries.get(index)),
            &self.reply,
        ) else {
            return;
        };
        cx.command(Command::Back);
        cx.command(Command::Emit(
            reply.clone(),
            Params::new().with(params::ID, id.as_str().to_string()),
        ));
    }

    /// Back out of a group to the groups, where the focus was; `false` where
    /// there is none to leave (the screen closes).
    fn leave_group(&mut self, cx: &mut Cx<'_, '_>) -> bool {
        match self.open {
            Some(group) if self.grouped => {
                self.open = None;
                self.show_list(group, cx);
                true
            }
            _ => false,
        }
    }

    /// The rows are other ones: from the top, the focus on `focus`.
    fn show_list(&mut self, focus: usize, cx: &mut Cx<'_, '_>) {
        self.list.set_focus(focus);
        cx.request_redraw();
        cx.request_refresh(RefreshMode::Partial);
    }
}

impl Screen for PickScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let len = self.len();
        self.header
            .draw(&mut ui, self.heading(), None, self.list.paging(len));
        let (groups, open) = (&self.groups, self.open);
        self.list.draw(&mut ui, len, |ui, index, rect, focused| {
            match open.and_then(|group| groups.get(group)) {
                Some(group) => ui::Row::value(&group.entries[index].1, "")
                    .focused(focused)
                    .draw(ui, rect),
                None => {
                    // A group shows how many it holds, and that it opens.
                    let held = alloc::format!("{} ›", groups[index].entries.len());
                    ui::Row::value(&groups[index].name, &held)
                        .focused(focused)
                        .draw(ui, rect);
                }
            }
        });
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        let len = self.len();
        let rows = self.list.rows() as isize;
        let delta = if let Some(delta) = ui::list_step(a, rows) {
            delta
        } else if *a == actions::CONFIRM {
            self.pick(self.list.focus(), cx);
            return Handled::Yes;
        } else if *a == actions::BACK && self.leave_group(cx) {
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list.move_focus(delta, len).request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let len = self.len();
        let paging = self.list.paging(len);
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, len) {
            Some(index) => {
                self.list.set_focus(index);
                self.pick(index, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(ids: &[&str]) -> Vec<(Id, String)> {
        ids.iter()
            .map(|id| (Id::parse(id).unwrap(), id.to_uppercase()))
            .collect()
    }

    fn owner(namespace: &str) -> String {
        match namespace {
            "files" => String::from("Library"),
            other => other.to_string(),
        }
    }

    #[test]
    fn what_fits_a_page_comes_as_one_list() {
        let (groups, grouped) = arrange(entries(&["b.x", "a.y", "a.z"]), 3, owner);
        assert!(!grouped);
        assert_eq!(groups.len(), 1);
        let names: Vec<_> = groups[0].entries.iter().map(|e| e.1.as_str()).collect();
        assert_eq!(names, ["A.Y", "A.Z", "B.X"], "by name");
    }

    #[test]
    fn one_owner_alone_is_no_group() {
        let (groups, grouped) = arrange(entries(&["a.x", "a.y", "a.z", "a.w"]), 2, owner);
        assert!(!grouped, "nothing to choose between");
        assert_eq!(groups[0].entries.len(), 4);
    }

    #[test]
    fn a_long_list_comes_by_owner_in_the_order_of_their_names() {
        let ids = ["files.b", "zed.a", "files.a", "beta.a", "zed.b"];
        let (groups, grouped) = arrange(entries(&ids), 2, owner);
        assert!(grouped);
        let names: Vec<_> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["beta", "Library", "zed"], "by the owner's name");
        let library: Vec<_> = groups[1].entries.iter().map(|e| e.1.as_str()).collect();
        assert_eq!(library, ["FILES.A", "FILES.B"], "each by name");
    }

    #[test]
    fn the_same_id_is_listed_once() {
        let mut list = entries(&["a.x", "b.y"]);
        list.push((Id::parse("a.x").unwrap(), String::from("A.X")));
        let (groups, _) = arrange(list, 10, owner);
        assert_eq!(groups[0].entries.len(), 2);
    }
}
