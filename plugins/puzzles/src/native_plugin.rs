//! The interface-7 Puzzles plugin: the screen (`native_view`, drawn by
//! `native_draw`) and the home widget *Puzzle of the day*. This file is the
//! glue: it reads the clock, keeps and loads the plugin's small files, reads
//! the crossword files of the card, asks for the letters of a word on the
//! keyboard, and tells the host which part of the screen to draw again.
//!
//! Nothing runs while no screen is open: the plugin has no work timers, and
//! the widget changes only when the puzzle of the day does.
use crate::{
    declarations::{DAILY_TOKEN, LIFECYCLE_TOKEN, OPEN_TOKEN, SCREEN_TOKEN},
    native_view::{Act, Redraw, ScreenView, dirty},
    native_widgets::{self, Shown},
    puz::{self, Crossword, FILE_MAX},
    state::{
        DAILY_FILE, FOLDER, KEPT_MAX, STATS_FILE, SUDOKU_FILE, Stats, crossword_file, is_crossword,
    },
    sudoku::{Game, Level},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect, Storage,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input},
        clock,
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered, Event, Incoming, kinds},
        files::Files,
        formatting::Arg,
        registrations,
        reply::Reply,
        snapshot,
        storage::{self, DataStorage, Root},
        ui::{
            Instance, Question, RequestId, Ui, WidgetInputKind, WidgetTarget, stages::WidgetCall,
        },
    },
};
use paperpaw_ui_kit::{
    Messages, Screens, paint, read_event,
    widget::{Cx, cx::zone_offset},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/puzzles_declarations.rs"));
}

/// The most bytes of a stage reply (a size and a target).
const REPLY: usize = 512;
/// The target of a widget: all of it.
const TARGET: u32 = 1;
/// The screen's id, for opening it.
const SCREEN: &str = "puzzles.screen";
/// The most crossword files listed.
const FILES_MAX: usize = 64;

/// A widget instance the host made.
#[derive(Debug)]
struct Item {
    instance: Instance,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when the host was told last.
    told: Option<Shown>,
}

/// The interface-7 Puzzles plugin.
#[derive(Debug, Default)]
pub struct NativePuzzles {
    stats: Stats,
    loaded: bool,
    items: Vec<Item>,
    screens: Screens<ScreenView>,
    /// The question for the letters of a word, while it is open.
    asked: Option<RequestId>,
    /// The next screen opens on the puzzle of the day.
    want_daily: bool,
    /// The screen just opened is to show it when it is on top.
    daily_first: bool,
}

impl Plugin for NativePuzzles {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::UI
            .iter()
            .chain(image::NAVIGATION)
            .chain(image::WIDGETS)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
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
        let Some(event) = read_event!(
            incoming;
            kinds::Started,
            kinds::Resume,
            kinds::SettingChanged,
            kinds::ActionRun,
            kinds::WidgetMade,
            kinds::WidgetDropped,
            kinds::StageCall,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Answered
        ) else {
            return Ok(());
        };
        match event {
            Event::Started { .. } | Event::Resume { .. } | Event::SettingChanged { .. }
                if token == LIFECYCLE_TOKEN =>
            {
                self.load_stats(host)?;
                self.tell_widgets(host, true)
            }
            Event::ActionRun { .. } if token == OPEN_TOKEN => open_screen(host),
            Event::ActionRun { .. } if token == DAILY_TOKEN => {
                self.want_daily = true;
                open_screen(host)
            }
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
            Event::ScreenOpened { instance, .. } if token == SCREEN_TOKEN => {
                self.screens.open(instance, ScreenView::new())?;
                self.opened(host, instance)
            }
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if token == SCREEN_TOKEN => {
                self.screens.shown(token, instance)?;
                self.shown(host)
            }
            Event::ScreenClosed { instance, .. } if token == SCREEN_TOKEN => {
                self.screens.close(instance);
                self.save_stats(host);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if token == SCREEN_TOKEN {
            let view = self.screens.top(token).ok_or(Error::State)?;
            return view.draw(host, &mut frame, &self.stats);
        }
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        self.load_stats(host)?;
        let shown = Shown::of(&self.stats, Self::day(host)?);
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = area_of(frame.rect);
        let mut out = Vec::new();
        native_widgets::draw(&mut cx, &mut out, area, &shown)?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == SCREEN_TOKEN {
            let Some(view) = self.screens.top(token) else {
                return Ok(Handled::No);
            };
            let act = view.input(&input, &mut self.stats);
            return self.act(host, act);
        }
        let Input::Widget {
            instance,
            target: TARGET,
            kind,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        self.find(instance.get()).ok_or(Error::NotFound)?;
        match kind {
            WidgetInputKind::Tap | WidgetInputKind::LongPress => {
                self.want_daily = true;
                open_screen(host)?;
                Ok(Handled::Yes)
            }
            _ => Ok(Handled::No),
        }
    }
}

impl NativePuzzles {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    /// Today's number (days since 1970 by the local time), if the device
    /// knows the time.
    fn day(host: &Host<'_>) -> Result<Option<i32>, Error> {
        let Some(unix) = snapshot::capture_snapshot(host)?.unix_seconds() else {
            return Ok(None);
        };
        let local = unix.saturating_add(i64::from(zone_offset(host, unix)?));
        Ok(i32::try_from(local.div_euclid(86_400)).ok())
    }

    // The kept files.

    fn read_kept(host: &Host<'_>, name: &str, room: usize) -> Option<String> {
        let mut buffer = alloc::vec![0u8; room + 1];
        let n = DataStorage::new(host).read_at(name, 0, &mut buffer).ok()?;
        let text = buffer.get(..n).filter(|t| t.len() <= room)?;
        String::from_utf8(text.to_vec()).ok()
    }

    fn write_kept(host: &Host<'_>, name: &str, text: &str) {
        DataStorage::new(host).write(name, text.as_bytes()).ok();
    }

    fn load_stats(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.loaded {
            return Ok(());
        }
        self.loaded = true;
        if let Some(stats) =
            Self::read_kept(host, STATS_FILE, KEPT_MAX).and_then(|t| Stats::parse(&t))
        {
            self.stats = stats;
        }
        Ok(())
    }

    fn save_stats(&self, host: &Host<'_>) {
        if self.loaded {
            Self::write_kept(host, STATS_FILE, &self.stats.text());
        }
    }

    /// The screen was opened: the games kept are loaded.
    fn opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.load_stats(host)?;
        let today = Self::day(host)?;
        let free = Self::read_kept(host, SUDOKU_FILE, 2 * KEPT_MAX).and_then(|t| Game::parse(&t));
        let daily = Self::read_kept(host, DAILY_FILE, 2 * KEPT_MAX)
            .and_then(|t| Game::parse(&t))
            .filter(|g| Some(g.day) == today);
        self.daily_first = core::mem::take(&mut self.want_daily);
        let Some(view) = self.screens.get(instance) else {
            return Ok(());
        };
        view.today = today;
        view.free = free;
        view.daily = daily;
        Ok(())
    }

    /// The screen is on top: where it was asked to open on the puzzle of the
    /// day, it does (once).
    fn shown(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if !core::mem::take(&mut self.daily_first) {
            return Ok(());
        }
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        let act = view.start_daily(&self.stats);
        self.act(host, act).map(|_| ())
    }

    // What the view asked for.

    fn act(&mut self, host: &Host<'_>, act: Act) -> Result<Handled, Error> {
        match act {
            Act::Ignored => return Ok(Handled::No),
            Act::Back => {
                let mut scratch = [0; 64];
                CommandSender::new(host, &mut scratch).back().ok();
            }
            Act::Draw(redraw) => draw_again(host, &redraw),
            Act::Generate { daily, level } => self.generate(host, daily, level)?,
            Act::ListFiles => self.list_files(host)?,
            Act::OpenFile(index) => self.open_file(host, index)?,
            Act::Type => self.ask_word(host)?,
        }
        self.keep(host)?;
        Ok(Handled::Yes)
    }

    /// Makes a puzzle and shows it.
    fn generate(&mut self, host: &Host<'_>, daily: bool, level: Level) -> Result<(), Error> {
        let today = Self::day(host)?;
        let (seed, day) = if daily {
            let day = today.unwrap_or(0);
            (
                u32::try_from(day)
                    .unwrap_or(0)
                    .wrapping_mul(7_919)
                    .wrapping_add(1),
                day,
            )
        } else {
            let micros = clock::micros(host)?;
            ((micros ^ (micros >> 20)) as u32, 0)
        };
        let game = Game::new(seed, if daily { Level::Medium } else { level }, day);
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        let act = view.start_sudoku(game, daily);
        if let Act::Draw(redraw) = act {
            draw_again(host, &redraw);
        }
        Ok(())
    }

    /// Lists the crossword files of the card's folder.
    fn list_files(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut names = Self::folder(host).unwrap_or_default();
        names.sort_by_key(|n| n.to_ascii_lowercase());
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        if let Act::Draw(redraw) = view.set_files(names) {
            draw_again(host, &redraw);
        }
        Ok(())
    }

    fn folder(host: &Host<'_>) -> Result<Vec<String>, Error> {
        let files = Files::open(host)?;
        let found = (|| {
            let mut names = Vec::new();
            let mut cursor = 0;
            loop {
                let (mut input, mut out) = (
                    paperpaw_ui_kit::mem::buffer(paperpaw_ui_kit::mem::SERVICE_BUFFER)?,
                    paperpaw_ui_kit::mem::buffer(paperpaw_ui_kit::mem::SERVICE_BUFFER)?,
                );
                let page = files.list(host, Root::Files, FOLDER, cursor, &mut input, &mut out)?;
                for entry in page.entries() {
                    if !entry.is_folder() && !entry.unaddressable() && is_crossword(entry.name()) {
                        names.try_reserve(1).map_err(|_| Error::NoMemory)?;
                        names.push(String::from(entry.name()));
                    }
                }
                match page.next() {
                    Some(next) if names.len() < FILES_MAX => cursor = next,
                    _ => return Ok::<_, Error>(names),
                }
            }
        })();
        let released = files.release(host);
        let names = found?;
        released?;
        Ok(names)
    }

    /// Reads the crossword file `index` of the list.
    fn open_file(&mut self, host: &Host<'_>, index: usize) -> Result<(), Error> {
        let Some(name) = self
            .screens
            .top(SCREEN_TOKEN)
            .and_then(|v| v.files.get(index).cloned())
        else {
            return Ok(());
        };
        let parsed = Self::read_crossword(host, &name);
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        let act = match parsed {
            Ok(mut cw) => {
                if let Some(kept) = Self::read_kept(host, &crossword_file(&name), 1024) {
                    cw.restore(&kept);
                }
                view.start_crossword(cw, name)
            }
            Err(fault) => view.fail(fault),
        };
        if let Act::Draw(redraw) = act {
            draw_again(host, &redraw);
        }
        Ok(())
    }

    fn read_crossword(host: &Host<'_>, name: &str) -> Result<Crossword, puz::Fault> {
        let path = [FOLDER, "/", name].concat();
        let size = storage::size(host, Root::Files, &path).map_err(|_| puz::Fault::Format)?;
        if size > FILE_MAX as u64 {
            return Err(puz::Fault::Large);
        }
        let mut data: Vec<u8> = Vec::new();
        data.try_reserve(usize::try_from(size).unwrap_or(0))
            .map_err(|_| puz::Fault::Large)?;
        let mut chunk = alloc::vec![0u8; 2048];
        loop {
            let at = data.len() as u64;
            let n = storage::read_at(host, Root::Files, &path, at, &mut chunk)
                .map_err(|_| puz::Fault::Format)?;
            if n == 0 {
                break;
            }
            data.extend_from_slice(chunk.get(..n).unwrap_or(&[]));
            if data.len() > FILE_MAX {
                return Err(puz::Fault::Large);
            }
        }
        Crossword::from_bytes(&data)
    }

    /// Asks for the letters of the word under the cursor on the keyboard.
    fn ask_word(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        let Some(game) = view.cross.as_ref() else {
            return Ok(());
        };
        let Some(clue) = game.cw.word_at(game.cursor, game.across) else {
            return Ok(());
        };
        let across = game.across;
        let word = game.cw.word_text(clue, across).replace('-', "_");
        let initial = if word.chars().all(|c| c == '_') {
            String::new()
        } else {
            word
        };
        let mut messages = Messages::new()?;
        let key = if across {
            "puzzles.clue.across"
        } else {
            "puzzles.clue.down"
        };
        let mut title = messages.format(
            host,
            key,
            &[Arg::Int {
                name: "number",
                value: i64::from(clue.number),
            }],
        )?;
        title.push_str(": ");
        title.push_str(&clue.text);
        let ui = Ui::open(host)?;
        let made = (|| {
            let mut input = paperpaw_ui_kit::mem::buffer(paperpaw_ui_kit::mem::SERVICE_BUFFER)?;
            let mut out = paperpaw_ui_kit::mem::buffer(paperpaw_ui_kit::mem::SERVICE_BUFFER)?;
            let request = ui.ask(host, Question::text(&title, "")?, &mut input, &mut out)?;
            if !initial.is_empty() {
                paperpaw_ui_kit::uitext::write_all(&ui, host, request.text(), &initial)?;
            }
            Ok::<_, Error>(request)
        })();
        let released = ui.release(host);
        self.asked = Some(made?);
        released
    }

    /// The letters typed: they go into the word.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some(request) = self.asked else {
            return Ok(());
        };
        if request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let ui = Ui::open(host)?;
        let typed = paperpaw_ui_kit::uitext::read_all(&ui, host, request.text(), 128);
        let released = ui.release(host);
        let typed = typed?;
        released?;
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        let act = view.typed(&typed, &mut self.stats);
        self.act(host, act).map(|_| ())
    }

    /// Keeps what changed on the screen, and tells the widgets.
    fn keep(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(view) = self.screens.top(SCREEN_TOKEN) else {
            return Ok(());
        };
        let changed = core::mem::take(&mut view.dirty);
        if changed == 0 {
            return Ok(());
        }
        view.sync_daily_to(&mut self.stats);
        let free = (changed & dirty::SUDOKU != 0)
            .then(|| view.free.as_ref().map(Game::text))
            .flatten();
        let daily = (changed & dirty::DAILY != 0)
            .then(|| view.daily.as_ref().map(Game::text))
            .flatten();
        let cross = (changed & dirty::CROSSWORD != 0)
            .then(|| {
                view.cross
                    .as_ref()
                    .map(|g| (crossword_file(&g.name), g.cw.text()))
            })
            .flatten();
        if let Some(text) = free {
            Self::write_kept(host, SUDOKU_FILE, &text);
        }
        if let Some(text) = daily {
            Self::write_kept(host, DAILY_FILE, &text);
        }
        if let Some((name, text)) = cross {
            Self::write_kept(host, &name, &text);
        }
        if changed & dirty::STATS != 0 {
            self.save_stats(host);
        }
        self.tell_widgets(host, false)
    }

    // The widget.

    fn made(&mut self, host: &Host<'_>, widget: &str, instance: Instance) -> Result<(), Error> {
        if paperpaw_ui_kit::widget::settings::base(widget) != "puzzles.daily" {
            return Ok(());
        }
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            widget: paperpaw_ui_kit::text::copy(widget)?,
            told: None,
        });
        self.load_stats(host)
    }

    /// Tells the widgets whose picture changed (all of them where `all`).
    fn tell_widgets(&mut self, host: &Host<'_>, all: bool) -> Result<(), Error> {
        if self.items.is_empty() {
            return Ok(());
        }
        let shown = Shown::of(&self.stats, Self::day(host)?);
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            if all || item.told != Some(shown) {
                commands.widget_changed(item.instance).ok();
                item.told = Some(shown);
            }
        }
        Ok(())
    }

    /// The widgets' stage: their height and where they sit.
    fn stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = paperpaw_ui_kit::mem::buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, .. } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let widget = paperpaw_ui_kit::text::copy(&item.widget)?;
                let cx = Cx::open(host, &widget)?;
                let height = native_widgets::height(&cx)?;
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
}

/// Asks the host to draw `redraw` again.
fn draw_again(host: &Host<'_>, redraw: &Redraw) {
    let mut scratch = [0; 64];
    let mut commands = CommandSender::new(host, &mut scratch);
    match redraw {
        Redraw::Areas(areas) => {
            for area in areas.iter().filter(|a| a.width > 0 && a.height > 0) {
                // A refusal (the screen is not on top) is not an error.
                commands
                    .queue_redraw_area(RedrawMode::Partial, paint::frame_rect(*area))
                    .ok();
            }
        }
        Redraw::Part => {
            commands.queue_redraw(RedrawMode::Partial).ok();
        }
        Redraw::Full => {
            commands.queue_redraw(RedrawMode::Full).ok();
        }
    }
}

/// The rectangle of a drawing as the widgets' own.
fn area_of(rect: FrameRect) -> Rect {
    let clamp = paperpaw_ui_kit::geometry::clamp_u16;
    Rect::new(
        clamp(i64::from(rect.x)),
        clamp(i64::from(rect.y)),
        clamp(i64::from(rect.width)),
        clamp(i64::from(rect.height)),
    )
}

/// Opens the screen.
fn open_screen(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open_screen(SCREEN)
}
