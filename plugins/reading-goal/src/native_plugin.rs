//! The interface-7 Reading goal: it asks the statistics for today's reading
//! (the stage `reading-goal.read-today`, which they answer from memory) when a
//! book closes or the device sleeps or stops, and writes it to `today.txt` in
//! the plugin's data folder, and asks it again each minute for the widget.
//! *Reading cat* (the widget) is here too; its picture and the sleep screen
//! wait for images. See `PORTING-7.md`.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::{CommandSender, PluginEnabled},
        events::{AnswerOutcome, Answered, BookKind, Event, Incoming, kinds},
        fonts::{Font, FontRole},
        registrations,
        registry::{PageCursor, Registry, RegistryPluginState},
        reply::Reply,
        stages::Stage,
        storage::DataStorage,
        ui::{
            Instance, Open, Param, Question, RequestId, Ui, WidgetInputKind, WidgetTarget,
            stages::WidgetCall,
        },
    },
};
use paperpaw_ui_kit::{
    Messages,
    mem::{SERVICE_BUFFER, buffer},
    paint, read_event,
    text::copy,
    widget::Cx,
};

use crate::model::{Kept, Statistics, parse_stage};
use crate::native_widget::{self, TARGET};
use crate::store;

mod image {
    include!(concat!(env!("OUT_DIR"), "/reading_goal_declarations.rs"));
}

/// The plugin's callback tokens (see `declarations.rs`).
const EVENTS: u32 = crate::declarations::EVENTS_TOKEN;
/// The stage this plugin owns and runs, by its full id.
const READ_TODAY: &str = "reading-goal.read-today";
/// What a reply of the stage can take (a day and the seconds: 8 bytes).
const REPLY: usize = Stage::out_len(8);

/// The minute tick's callback token, as declared.
const TICK: u32 = crate::declarations::TICK_TOKEN;
/// The slot the settings fill, and the store's action.
const SETTINGS_SLOT: &str = "core.settings";
const STORE: &str = "store.open";
/// The reading statistics' plugin and the goal setting a tap opens.
const STATISTICS_ID: &str = "stats";
const GOAL: &str = "stats.goal";
/// The most pages of plugins read to find the statistics.
const MOST_PAGES: usize = 16;
/// The most bytes of a stage reply (sizes and up to 32 targets).
const WIDGET_REPLY: usize = 1280;

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when it was last drawn or asked about, for the tick.
    shown: Option<u64>,
}

/// Reading goal's interface-7 state.
#[derive(Debug)]
pub struct NativeReadingGoal {
    kept: Kept,
    /// Whether the statistics count, as read each minute (and when a widget
    /// is made); counting until read.
    statistics: Statistics,
    items: Vec<Item>,
    /// The offer of the statistics, if open: its request and the state it
    /// was made for.
    asked: Option<(RequestId, Statistics)>,
}

impl Default for NativeReadingGoal {
    fn default() -> Self {
        Self {
            kept: Kept::new(),
            statistics: Statistics::Counting,
            items: Vec::new(),
            asked: None,
        }
    }
}

impl Plugin for NativeReadingGoal {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::EVENTS)
            .chain(image::WIDGETS)
        {
            registrations::submit(host, batch)?;
        }
        // A host that does not admit stages yet leaves the plugin without
        // the figure; the rest works.
        for batch in image::STAGES {
            match registrations::submit(host, batch) {
                Ok(()) | Err(Error::Unsupported) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::BookOpened, kinds::BookClosed, kinds::Suspend, kinds::Stopping, kinds::WidgetMade, kinds::WidgetDropped, kinds::StageCall, kinds::Tick, kinds::Answered)
        else {
            return Ok(());
        };
        match event {
            Event::WidgetMade {
                widget, instance, ..
            } => self.made(host, widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::Tick { .. } if token == TICK => self.tick(host),
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => {
                let Some(edge) = edge(token, &event) else {
                    return Ok(());
                };
                // Asked when the figure is about to be written, not at every page.
                if matches!(edge, Edge::BookClosed | Edge::Sleeping)
                    && let Some((day, seconds)) = ask(host)
                {
                    self.kept.apply(Edge::Told(day, seconds));
                }
                if self.kept.apply(edge) {
                    self.flush(host);
                }
                Ok(())
            }
        }
    }

    fn draw(
        &mut self,
        host: &Host<'_>,
        _token: u32,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = Rect::new(
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.x)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.y)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.width)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.height)),
        );
        let today = native_widget::facts(host, &self.kept, self.statistics)?;
        let mut out = Vec::new();
        native_widget::draw(&mut cx, &mut out, area, &today)?;
        let bold = if out
            .iter()
            .any(|op| matches!(op, paperpaw_ui_kit::Paint::Bold { .. }))
        {
            Some(Font::acquire_role(host, FontRole::UiBold)?)
        } else {
            None
        };
        paint::replay_with(&mut frame, &cx.font, bold.as_ref(), &out)
    }

    fn input(&mut self, host: &Host<'_>, _token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Input::Widget {
            instance,
            target: TARGET,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        self.find(instance.get()).ok_or(Error::NotFound)?;
        self.tapped(host)?;
        Ok(Handled::Yes)
    }
}

/// The plugin that answers: the reading statistics.
const STATISTICS: &str = "stats";

/// Asks the statistics for today's reading. Nothing where they do not answer:
/// not running yet (they start afterwards, the next ask is answered), no stage
/// for them, a day they do not know, or an answer that is not a figure. Every
/// subscriber is read to the end, and only the statistics' reply counts: any
/// other plugin on the stage is not heard.
fn ask(host: &Host<'_>) -> Option<(i32, u32)> {
    let stage = Stage::new(host, READ_TODAY).ok()?;
    let mut out = [0u8; REPLY];
    let mut run = stage.run(&[], &mut out);
    let mut figure_told = None;
    // At most 32 subscribers are active on a stage (docs/abi/stages-services.md).
    for _ in 0..33 {
        let Ok(Some(reply)) = run.next() else {
            break;
        };
        if let Some(told) = answer_of(reply.subscriber(), reply.status(), reply.payload()) {
            figure_told = Some(told);
        }
    }
    figure_told
}

/// The figure a subscriber's reply holds: only the statistics', and only a
/// successful, well-formed one.
pub(crate) fn answer_of(
    subscriber: &str,
    status: Result<(), Error>,
    payload: &[u8],
) -> Option<(i32, u32)> {
    if subscriber != STATISTICS {
        return None;
    }
    figure(status, payload)
}

/// The figure a reply holds.
pub(crate) fn figure(status: Result<(), Error>, payload: &[u8]) -> Option<(i32, u32)> {
    status.ok()?;
    parse_stage(payload)
}

/// What happened, as far as the figure is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Edge {
    /// The statistics answered with today's seconds.
    Told(i32, u32),
    BookOpened,
    BookClosed,
    /// The device sleeps or stops.
    Sleeping,
}

/// The edge an event at `token` is, if it is one.
pub(crate) fn edge(token: u32, event: &Event<'_>) -> Option<Edge> {
    match (token, event) {
        (
            EVENTS,
            Event::Book {
                kind: BookKind::Opened,
                ..
            },
        ) => Some(Edge::BookOpened),
        (
            EVENTS,
            Event::Book {
                kind: BookKind::Closed,
                ..
            },
        ) => Some(Edge::BookClosed),
        (EVENTS, Event::Suspend { .. } | Event::Stopping { .. }) => Some(Edge::Sleeping),
        _ => None,
    }
}

impl Kept {
    /// Takes `edge` in; whether to write `today.txt` now. Not while pages turn:
    /// the card is slow to write to.
    pub(crate) fn apply(&mut self, edge: Edge) -> bool {
        match edge {
            Edge::Told(day, seconds) => self.read_today(day, seconds),
            Edge::BookOpened => {
                self.book_opened();
                false
            }
            Edge::BookClosed => self.book_closed(),
            Edge::Sleeping => self.must_flush(),
        }
    }
}

impl NativeReadingGoal {
    /// Writes what was told where the file holds something else. A failed
    /// write is not told: the figure is kept and written at the next edge, as
    /// on interface 6.
    fn flush(&self, host: &Host<'_>) {
        store::flush(&DataStorage::new(host), &self.kept).ok();
    }
}

impl NativeReadingGoal {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        if paperpaw_ui_kit::widget::settings::base(widget) != crate::declarations::WIDGET_GROUP {
            return Ok(());
        }
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            widget: copy(widget)?,
            shown: None,
        });
        self.statistics = read_statistics(host).unwrap_or(self.statistics);
        Ok(())
    }

    /// The host asks for the widget's height, where it sits and what a tap
    /// meets.
    fn stage(&self, host: &Host<'_>, payload: &[u8], reply: &mut Reply<'_>) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(WIDGET_REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let height = native_widget::height(&mut cx, width)?;
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_targets(&[WidgetTarget::new(TARGET, rect)], &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Each minute: the statistics' answer for today (kept, and written where
    /// it differs, as at a book closed), whether they count, and tells the
    /// makers whose widget shows something new. A widget that fails is left
    /// for the next tick and costs the others nothing.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.items.is_empty() {
            return Ok(());
        }
        if let Some((day, seconds)) = ask(host)
            && self.kept.apply(Edge::Told(day, seconds))
        {
            self.flush(host);
        }
        self.statistics = read_statistics(host).unwrap_or(self.statistics);
        let (kept, statistics) = (self.kept, self.statistics);
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let instance = item.instance;
            let shown = native_widget::shows(host, &kept, statistics);
            settle(&mut item.shown, shown, || commands.widget_changed(instance));
        }
        Ok(())
    }

    /// A tap: the goal's setting where the statistics count, else the offer
    /// of switching them on or getting them. One offer at a time: the first
    /// is still open and its answer is the one that counts.
    fn tapped(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(labels) = native_widget::offer(self.statistics) else {
            return open_goal(host);
        };
        if self.asked.is_none() {
            let request = ask_offer(host, &labels)?;
            self.asked = Some((request, self.statistics));
        }
        Ok(())
    }

    /// The answer to the offer: switch the statistics on (at the next start,
    /// so started anew at once) or go to the store.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((_, state)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered || answer.choice() != 0 {
            return Ok(());
        }
        let mut scratch = [0; 256];
        let mut commands = CommandSender::new(host, &mut scratch);
        match state {
            Statistics::Off => {
                commands.set_plugin_enabled(STATISTICS_ID, PluginEnabled::On)?;
                // Where the shell does not allow a restart the plugin is
                // still switched on, for the next start.
                commands.restart().ok();
                Ok(())
            }
            Statistics::Missing => commands.run_action(STORE),
            Statistics::Counting => Ok(()),
        }
    }
}

/// Notes what a widget shows now. If it differs from what it showed before,
/// `queue` tells the maker, and only once that is queued is it remembered as
/// shown; a refusal of either leaves `shown` as it was, so the next tick
/// tries again.
fn settle(
    shown: &mut Option<u64>,
    now: Result<u64, Error>,
    queue: impl FnOnce() -> Result<(), Error>,
) {
    let Ok(now) = now else {
        return;
    };
    if shown.is_some_and(|before| before != now) && queue().is_err() {
        return;
    }
    *shown = Some(now);
}

/// Whether the statistics count: running, off (or failed), or not installed.
fn read_statistics(host: &Host<'_>) -> Result<Statistics, Error> {
    let registry = Registry::open(host)?;
    let found = (|| {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        let mut cursor = PageCursor::start();
        for _ in 0..MOST_PAGES {
            let page = registry.plugins(host, cursor, &mut input, &mut out)?;
            if let Some(plugin) = page.rows().find(|plugin| plugin.id() == STATISTICS_ID) {
                let running = plugin.state() == RegistryPluginState::Running;
                return Ok(Statistics::of(Some(running)));
            }
            match page.end().next_cursor() {
                Some(next) => cursor = next,
                None => return Ok(Statistics::of(None)),
            }
        }
        Err(Error::NoSpace)
    })();
    let released = registry.release(host);
    let found = found?;
    released?;
    Ok(found)
}

/// Opens the settings on the goal.
fn open_goal(host: &Host<'_>) -> Result<(), Error> {
    let ui = Ui::open(host)?;
    let made = (|| {
        let (mut input, mut out) = (buffer(1024)?, buffer(1024)?);
        ui.handover(host, &[Param::new("focus", GOAL)?], &mut input, &mut out)
    })();
    let released = ui.release(host);
    let handover = made?;
    released?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot(SETTINGS_SLOT)?.handover(handover))
}

/// Asks which of `labels` (translation keys, one per line of the choices).
fn ask_offer(host: &Host<'_>, labels: &[&str; 2]) -> Result<RequestId, Error> {
    let mut messages = Messages::new()?;
    let title = messages.copy(host, "reading-goal.offer")?;
    let first = messages.copy(host, labels[0])?;
    let second = messages.copy(host, labels[1])?;
    let choices = paperpaw_ui_kit::text::concat(&[&first, "\n", &second])?;
    let ui = Ui::open(host)?;
    let made = (|| {
        let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
        ui.ask(
            host,
            Question::choice(&title, "", &choices)?,
            &mut input,
            &mut out,
        )
    })();
    let released = ui.release(host);
    let request = made?;
    released?;
    Ok(request)
}

#[cfg(test)]
mod tests;
