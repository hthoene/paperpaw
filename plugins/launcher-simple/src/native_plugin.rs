//! The interface-7 simple launcher: the home screen, the quick panel, the
//! library and the page of the editors, as one plugin.
//!
//! What the screens show is read once and kept (the cards, the dock and the
//! panel from the registry, the books from the library, the time at each
//! minute's tick): a key press moves the focus and redraws the two parts it
//! touches, nothing more. What changes what is known (the registry, a
//! setting, the library, a screen shown again) makes the next frame read it
//! anew. What the plugin decides without a host is in `model`; the drawing is
//! in `home`, `panel`, `library` and `customise` (`PORTING-7.md`).
use crate::{
    data::{self, Book, Reader, Row, Setting},
    declarations::{
        ACTION_CONTINUE, CONTINUE, CUSTOMISE, HOME, LIBRARY, LIBRARY_CHANGED, MENU_CARDS,
        MENU_DOCK, MENU_PANEL, OPEN_CUSTOMISE, PANEL, REGISTRY_CHANGED, SETTING_CHANGED,
        SHOW_BATTERY, SHOW_DATE, TICK,
    },
    home, library,
    look::{B, Ui},
    model::{self, Dir, Target, Zone},
    panel::{self, Bar, Toggle},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, FrameRect, Input, InputGestureKind},
        commands::{CommandSender, RedrawMode},
        events::{Event, Incoming, kinds},
        fonts::{Font, FontRole},
        registrations,
        reply::Reply,
        settings::{SettingValue, read_setting},
        snapshot,
        ui::{HandoverId, Instance, Open, Param, Ui as HostUi},
    },
    time::LocalTime,
};
use paperpaw_ui_kit::{
    Screens, Style, icons,
    mem::{SERVICE_BUFFER, buffer},
    paint, read_event,
    text::copy,
    widget::cx::local_time_now,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/launcher_simple_declarations.rs"));
}

/// The parameter of the quick panel: the menu it shows.
const PARAM_MENU: &str = "menu";
/// The menu the core opens the panel with.
const CORE_QUICK: &str = "core.quick";
/// How far the focus ring of a round part or a bar reaches outside it, in pixels.
const REACH: i32 = 20;
/// The menu editor's screen of one menu's entries and its parameter.
const EDITOR: &str = "menu-editor.entries";

/// What a screen is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Home,
    Panel,
    Library,
    Customise,
}

fn kind_of(token: u32) -> Option<Kind> {
    match token {
        HOME => Some(Kind::Home),
        PANEL => Some(Kind::Panel),
        LIBRARY => Some(Kind::Library),
        CUSTOMISE => Some(Kind::Customise),
        _ => None,
    }
}

/// An open screen.
#[derive(Debug)]
struct View {
    kind: Kind,
    focus: usize,
    /// What the last drawing put where, in screen pixels.
    zones: Vec<Zone>,
    /// The top left of the screen's part of the display.
    origin: (i32, i32),
    /// The library's page and what it held.
    page: usize,
    drawn: library::Drawn,
    /// The status line and the clock, for the minute's tick.
    clock: Option<Rect>,
    /// The panel: the menu it shows and what it made of it.
    menu: String,
    panel: Option<panel::Model>,
}

impl View {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            focus: 0,
            zones: Vec::new(),
            origin: (0, 0),
            page: 0,
            drawn: library::Drawn::default(),
            clock: None,
            menu: String::new(),
            panel: None,
        }
    }
}

/// The plugin.
#[derive(Debug, Default)]
pub struct NativeLauncher {
    ui: Option<HostUi>,
    screens: Screens<View>,
    cards: Option<Vec<Row>>,
    dock: Option<Vec<Row>>,
    /// The book opened last, if there is one (`Some(None)`: asked, none).
    recent: Option<Option<Book>>,
    books: Option<Vec<Book>>,
    /// The minute the home screen last showed (hour * 60 + minute).
    minute: Option<u16>,
}

impl Plugin for NativeLauncher {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS
            .iter()
            .chain(image::UI)
            .chain(image::NAVIGATION)
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
        _reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(
            incoming;
            kinds::ActionRun,
            kinds::ScreenOpened,
            kinds::ScreenShown,
            kinds::ScreenClosed,
            kinds::Tick,
            kinds::SettingChanged,
            kinds::RegistryChanged,
            kinds::LibraryChanged
        ) else {
            return Ok(());
        };
        match event {
            Event::ActionRun { .. } if token == CONTINUE => self.continue_reading(host),
            Event::ActionRun { .. } if token == OPEN_CUSTOMISE => {
                let mut scratch = [0; 256];
                CommandSender::new(host, &mut scratch)
                    .open(Open::screen("launcher-simple.customise")?)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } if kind_of(token).is_some() => self.opened(host, token, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } if kind_of(token).is_some() => {
                self.screens.shown(token, instance)?;
                // Shown again (what covered it closed): the books may have moved.
                if matches!(kind_of(token), Some(Kind::Home | Kind::Library)) {
                    self.recent = None;
                    self.books = None;
                }
                Ok(())
            }
            Event::ScreenClosed { instance, .. } if kind_of(token).is_some() => {
                self.screens.close(instance);
                Ok(())
            }
            Event::Tick { .. } if token == TICK => self.tick(host),
            Event::SettingChanged { .. } if token == SETTING_CHANGED => self.changed(host),
            Event::RegistryChanged { .. } if token == REGISTRY_CHANGED => {
                self.cards = None;
                self.dock = None;
                self.changed(host)
            }
            Event::LibraryChanged { .. } if token == LIBRARY_CHANGED => {
                self.recent = None;
                self.books = None;
                self.redraw_top(host, Kind::Home)?;
                self.redraw_top(host, Kind::Library)
            }
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let kind = kind_of(token).ok_or(Error::Invalid)?;
        self.load(host, kind)?;
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let icon_font = icons::font(host)?;
        let drawn = self.paint(
            host,
            token,
            kind,
            &mut frame,
            &display,
            &font,
            icon_font.as_ref(),
        );
        if let Some(icon_font) = icon_font {
            icon_font.release()?;
        }
        font.release()?;
        drawn
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(kind) = kind_of(token) else {
            return Ok(Handled::No);
        };
        let Some(view) = self.screens.top(token) else {
            return Ok(Handled::No);
        };
        let at = match input {
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                let point = Point {
                    x: position.x + view.origin.0,
                    y: position.y + view.origin.1,
                };
                let Some(at) = model::hit(&view.zones, point) else {
                    return Ok(Handled::No);
                };
                view.focus = at;
                if let Some(Zone {
                    target: Target::Bar(index),
                    rect,
                }) = view.zones.get(at).copied()
                {
                    return self.set_bar_at(host, token, index, point.x, rect);
                }
                Step::Use(at)
            }
            Input::Action { action, .. } => match action {
                "core.confirm" => Step::Use(view.focus),
                "core.next-page" if kind == Kind::Library => Step::Turn(true),
                "core.prev-page" if kind == Kind::Library => Step::Turn(false),
                other => match Dir::of(other) {
                    Some(dir) => Step::Go(dir),
                    None => return Ok(Handled::No),
                },
            },
            _ => return Ok(Handled::No),
        };
        match at {
            // An action or screen that is not there (a plugin off) does nothing.
            Step::Use(index) => match self.activate(host, token, index) {
                Err(Error::NotFound) => {}
                other => other?,
            },
            Step::Turn(forward) => self.turn(host, token, forward)?,
            Step::Go(dir) => self.go(host, token, dir)?,
        }
        Ok(Handled::Yes)
    }
}

/// What an input means to a screen.
#[derive(Clone, Copy, Debug)]
enum Step {
    /// The part `n` is chosen.
    Use(usize),
    /// The library's page before or after.
    Turn(bool),
    /// The focus goes.
    Go(Dir),
}

impl NativeLauncher {
    fn ui(&mut self, host: &Host<'_>) -> Result<&HostUi, Error> {
        if self.ui.is_none() {
            self.ui = Some(HostUi::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// A screen was opened: what the panel is to show comes with its handover.
    fn opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        let kind = kind_of(token).ok_or(Error::Invalid)?;
        let mut view = View::new(kind);
        if kind == Kind::Panel {
            let mut menu = String::new();
            if let Some(handover) = handover {
                let (mut input, mut output) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                let ui = self.ui(host)?;
                for param in ui.params(host, handover, &mut input, &mut output)? {
                    let param = param?;
                    if param.key() == PARAM_MENU {
                        menu = copy(param.value())?;
                    }
                }
            }
            // The core's quick panel is this one's menu; another menu is shown as it is.
            view.menu = if menu.is_empty() || menu == CORE_QUICK {
                copy(MENU_PANEL)?
            } else {
                menu
            };
            view.panel = Some(panel_model(host, &view.menu)?);
        }
        self.screens.open(instance, view)
    }

    /// Reads what the screen `kind` shows and the dock, where it is not known.
    fn load(&mut self, host: &Host<'_>, kind: Kind) -> Result<(), Error> {
        if matches!(kind, Kind::Customise) {
            return Ok(());
        }
        if self.dock.is_none() {
            self.dock = Some(menu_rows(host, MENU_DOCK)?);
        }
        match kind {
            Kind::Home => {
                if self.cards.is_none() {
                    self.cards = Some(menu_rows(host, MENU_CARDS)?);
                }
                if self.recent.is_none() {
                    self.recent = Some(data::books(host, 1)?.into_iter().find(|b| b.opened));
                }
            }
            Kind::Library => {
                if self.books.is_none() {
                    self.books = Some(data::books(host, data::MOST_BOOKS)?);
                }
            }
            // The panel's model is made when it opens and when a setting changes.
            Kind::Panel | Kind::Customise => {}
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments, reason = "one drawing, its host's parts")]
    fn paint(
        &mut self,
        host: &Host<'_>,
        token: u32,
        kind: Kind,
        frame: &mut DrawFrame<'_>,
        display: &snapshot::Snapshot,
        font: &Font<'_>,
        icon_font: Option<&Font<'_>>,
    ) -> Result<(), Error> {
        let mut style = Style::read(host, display, font)?;
        style.describing = paint::asking(frame);
        let Self {
            screens,
            cards,
            dock,
            recent,
            books,
            minute,
            ..
        } = self;
        let view = screens.top(token).ok_or(Error::State)?;
        let pane = B::new(
            frame.rect.x,
            frame.rect.y,
            i32::try_from(frame.rect.width).unwrap_or(i32::MAX),
            i32::try_from(frame.rect.height).unwrap_or(i32::MAX),
        );
        let mut ui = Ui::new(host, (font, icon_font), &style, pane, view.focus)?;
        let all = ui.screen;
        let paper = ui.paper;
        ui.fill(all, paper)?;
        let none: Vec<Row> = Vec::new();
        let dock_rows = dock.as_deref().unwrap_or(&none);
        match kind {
            Kind::Home => {
                let time = local_time_now(host)?;
                let twelve = twelve_hours(host)?;
                let data = home::Data {
                    cards: cards.as_deref().unwrap_or(&none),
                    dock: dock_rows,
                    book: recent.as_ref().and_then(Option::as_ref),
                    time,
                    twelve_hours: twelve,
                    show_date: switch(host, SHOW_DATE)?,
                    battery: if switch(host, SHOW_BATTERY)? {
                        display.battery_percent()
                    } else {
                        None
                    },
                };
                *minute = time.map(minute_of);
                let drawn = home::draw(&mut ui, &data)?;
                view.clock = Some(drawn.clock);
            }
            Kind::Panel => {
                let title = ui.t("launcher-simple.panel.title")?;
                let model = view.panel.as_mut().ok_or(Error::State)?;
                panel::draw(&mut ui, model, &title, dock_rows)?;
            }
            Kind::Library => {
                let books = books.as_deref().unwrap_or(&[]);
                view.drawn = library::draw(&mut ui, books, view.page, dock_rows)?;
                view.page = view.page.min(view.drawn.pages.saturating_sub(1));
            }
            Kind::Customise => crate::customise::draw(&mut ui)?,
        }
        paint::replay_icons(frame, font, icon_font, ui.ops.iter())?;
        view.origin = (frame.rect.x, frame.rect.y);
        view.zones = core::mem::take(&mut ui.zones);
        view.focus = view.focus.min(view.zones.len().saturating_sub(1));
        Ok(())
    }

    /// The minute's tick: the status line and the clock of the home screen
    /// are drawn again where the minute changed, and nothing else.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(time) = local_time_now(host)? else {
            return Ok(());
        };
        if self.minute == Some(minute_of(time)) {
            return Ok(());
        }
        let Some(view) = self.screens.top(HOME) else {
            return Ok(());
        };
        let Some(clock) = view.clock else {
            return Ok(());
        };
        let area = area_of(view.origin, clock, 0);
        let mut scratch = [0; 256];
        // A refusal: the home screen is not the one on top, and the next frame shows the time.
        let _ = CommandSender::new(host, &mut scratch).queue_redraw_area(RedrawMode::Partial, area);
        Ok(())
    }

    /// A setting changed: the panel's switches and bars are read anew.
    fn changed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut any = false;
        for view in self.screens.all() {
            if view.kind == Kind::Panel {
                view.panel = Some(panel_model(host, &view.menu)?);
                any = true;
            }
        }
        if any {
            self.redraw_top(host, Kind::Panel)?;
        }
        Ok(())
    }

    /// The screen of `kind` on top is drawn anew (a partial refresh).
    fn redraw_top(&mut self, host: &Host<'_>, kind: Kind) -> Result<(), Error> {
        let token = match kind {
            Kind::Home => HOME,
            Kind::Panel => PANEL,
            Kind::Library => LIBRARY,
            Kind::Customise => CUSTOMISE,
        };
        if self.screens.top(token).is_none() {
            return Ok(());
        }
        let mut scratch = [0; 64];
        let _ = CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial);
        Ok(())
    }

    /// The focus goes in `dir`: the two parts it touches are drawn again.
    fn go(&mut self, host: &Host<'_>, token: u32, dir: Dir) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let from = view.focus.min(view.zones.len().saturating_sub(1));
        let Some(zone) = view.zones.get(from).copied() else {
            return Ok(());
        };
        // Down from the last book of a page turns the page, up from the first.
        if view.kind == Kind::Library {
            let last = view.drawn.shown.saturating_sub(1);
            let books_end = matches!(zone.target, Target::Book(n) if n == last);
            let books_start = matches!(zone.target, Target::Book(0));
            if books_end && matches!(dir, Dir::Down | Dir::Next) && view.page + 1 < view.drawn.pages
            {
                return self.turn(host, token, true);
            }
            if books_start && matches!(dir, Dir::Up | Dir::Previous) && view.page > 0 {
                self.turn(host, token, false)?;
                if let Some(view) = self.screens.top(token) {
                    view.focus = view.drawn.per_page.saturating_sub(1);
                }
                return Ok(());
            }
        }
        let Some(to) = model::step(&view.zones, from, dir) else {
            return Ok(());
        };
        if to == from {
            return Ok(());
        }
        view.focus = to;
        let reach = REACH;
        let area = match view.zones.get(to) {
            Some(target) => zone.rect.union(&target.rect),
            None => zone.rect,
        };
        let area = area_of(view.origin, area, reach);
        let mut scratch = [0; 256];
        CommandSender::new(host, &mut scratch).queue_redraw_area(RedrawMode::Partial, area)
    }

    /// The library shows the page after or before.
    fn turn(&mut self, host: &Host<'_>, token: u32, forward: bool) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let page = if forward {
            (view.page + 1).min(view.drawn.pages.saturating_sub(1))
        } else {
            view.page.saturating_sub(1)
        };
        if page == view.page {
            return Ok(());
        }
        view.page = page;
        view.focus = 0;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }

    /// A bar's value, a tenth of its range more, from the end back to the
    /// start (Select: the arrow keys move the focus).
    fn bump_bar(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let Some(bar) = self.bar_of(token, index) else {
            return Ok(());
        };
        let n = &bar.number;
        let value = if n.value >= n.max {
            n.min
        } else {
            model::stepped(
                n.value,
                (n.min, n.max, ((n.max - n.min) / 10).max(n.step)),
                true,
            )
        };
        self.write_bar(host, token, index, value)
    }

    /// A bar's value at the finger (a tap).
    fn set_bar_at(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        x: i32,
        rect: Rect,
    ) -> Result<Handled, Error> {
        let Some(bar) = self.bar_of(token, index) else {
            return Ok(Handled::No);
        };
        let range = (bar.number.min, bar.number.max, bar.number.step);
        // The pill's ends are its round caps: the value spans what is between.
        let cap = i32::from(rect.height) / 2;
        let left = i32::from(rect.x) + cap;
        let width = i32::from(rect.width) - 2 * cap;
        let value = model::from_tap(x, (left, width), range);
        self.write_bar(host, token, index, value)?;
        Ok(Handled::Yes)
    }

    fn bar_of(&mut self, token: u32, index: usize) -> Option<Bar> {
        let view = self.screens.top(token)?;
        view.panel.as_ref()?.bars.get(index).cloned()
    }

    /// Sets the bar's setting and shows the value now: the setting's change
    /// comes back as an event that makes the panel read it anew.
    fn write_bar(
        &mut self,
        host: &Host<'_>,
        token: u32,
        index: usize,
        value: i64,
    ) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let Some(bar) = view.panel.as_mut().and_then(|p| p.bars.get_mut(index)) else {
            return Ok(());
        };
        if bar.number.value == value {
            return Ok(());
        }
        bar.number.value = value;
        let mut scratch = [0; 256];
        let mut commands = CommandSender::new(host, &mut scratch);
        commands.set_setting(&bar.row.item, SettingValue::Int(value))?;
        let area = view
            .zones
            .iter()
            .find(|z| z.target == Target::Bar(index))
            .map(|z| z.rect);
        let area = area_of(view.origin, area.unwrap_or(Rect::new(0, 0, 0, 0)), REACH);
        commands.queue_redraw_area(RedrawMode::Partial, area)
    }

    /// A part is chosen: what it does.
    fn activate(&mut self, host: &Host<'_>, token: u32, index: usize) -> Result<(), Error> {
        let view = self.screens.top(token).ok_or(Error::State)?;
        let Some(zone) = view.zones.get(index).copied() else {
            return Ok(());
        };
        let kind = view.kind;
        let mut scratch = [0; 256];
        match zone.target {
            Target::Card(n) => {
                let Some(row) = self.cards.as_ref().and_then(|c| c.get(n)).cloned() else {
                    return Ok(());
                };
                if row.item == ACTION_CONTINUE {
                    self.continue_reading(host)
                } else {
                    run_row(host, &row, &mut scratch)
                }
            }
            Target::Dock(n) => {
                let Some(row) = self.dock.as_ref().and_then(|d| d.get(n)).cloned() else {
                    return Ok(());
                };
                // The icon of the screen shown does nothing.
                let here = match kind {
                    Kind::Home => home::HOME,
                    Kind::Panel => panel::PANEL,
                    Kind::Library => library::LIBRARY,
                    Kind::Customise => "",
                };
                if row.item == here {
                    return Ok(());
                }
                run_row(host, &row, &mut scratch)
            }
            Target::Toggle(n) => {
                let Some(toggle) = view.panel.as_ref().and_then(|p| p.toggles.get(n)).cloned()
                else {
                    return Ok(());
                };
                match toggle.row.kind {
                    data::Kind::Setting => {
                        // A switch: the setting turns over; the change comes back as an event.
                        CommandSender::new(host, &mut scratch)
                            .set_setting(&toggle.row.item, SettingValue::Bool(!toggle.row.on))
                    }
                    _ => {
                        // Something to open or do: the panel is left first.
                        let mut commands = CommandSender::new(host, &mut scratch);
                        commands.close_screen()?;
                        commands.run_action(&toggle.row.item)
                    }
                }
            }
            Target::Bar(n) => self.bump_bar(host, token, n),
            Target::Book(n) => {
                let at = view.page * view.drawn.per_page + n;
                let Some(book) = self.books.as_ref().and_then(|b| b.get(at)).cloned() else {
                    return Ok(());
                };
                let ui = self.ui(host)?;
                data::open_book(host, ui, &book)
            }
            Target::Edit(n) => {
                let Some((menu, ..)) = crate::customise::ENTRIES.get(n) else {
                    return Ok(());
                };
                let (mut input, mut out) = (buffer(SERVICE_BUFFER)?, buffer(SERVICE_BUFFER)?);
                let params = [Param::new("menu", menu)?];
                let handover = self
                    .ui(host)?
                    .handover(host, &params, &mut input, &mut out)?;
                CommandSender::new(host, &mut scratch)
                    .open(Open::screen(EDITOR)?.handover(handover))
            }
            Target::Page(forward) => self.turn(host, token, forward),
        }
    }

    /// Opens the book opened last (the card *Continue reading*); with none, the library.
    fn continue_reading(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.recent.is_none() {
            self.recent = Some(data::books(host, 1)?.into_iter().find(|b| b.opened));
        }
        let book = self.recent.clone().flatten();
        match book {
            Some(book) => {
                let ui = self.ui(host)?;
                data::open_book(host, ui, &book)
            }
            None => {
                let mut scratch = [0; 256];
                CommandSender::new(host, &mut scratch).run_action("core.library")
            }
        }
    }
}

/// Runs the action (or opens the menu) of a row.
fn run_row(host: &Host<'_>, row: &Row, scratch: &mut [u8]) -> Result<(), Error> {
    CommandSender::new(host, scratch).run_action(&row.item)
}

/// The rows of `menu` that can be chosen now.
fn menu_rows(host: &Host<'_>, menu: &str) -> Result<Vec<Row>, Error> {
    let mut reader = Reader::open(host)?;
    let rows = reader.menu(menu);
    reader.finish(rows)
}

/// The panel's switches and bars from the rows of `menu`.
fn panel_model(host: &Host<'_>, menu: &str) -> Result<panel::Model, Error> {
    let mut reader = Reader::open(host)?;
    let made = (|| {
        let rows = reader.menu(menu)?;
        let (mut toggles, mut bars) = (Vec::new(), Vec::new());
        for mut row in rows {
            if row.faint {
                continue;
            }
            match row.kind {
                data::Kind::Setting => match reader.setting(&row.item)? {
                    Some(Setting::Switch(on)) => {
                        row.on = on;
                        paperpaw_ui_kit::mem::push(&mut toggles, Toggle { row })?;
                    }
                    Some(Setting::Number(number)) => {
                        paperpaw_ui_kit::mem::push(&mut bars, Bar { row, number })?;
                    }
                    _ => {}
                },
                _ => paperpaw_ui_kit::mem::push(&mut toggles, Toggle { row })?,
            }
        }
        Ok(panel::Model::new(toggles, bars))
    })();
    reader.finish(made)
}

/// A switch setting of the plugin; on where it cannot be read.
fn switch(host: &Host<'_>, key: &str) -> Result<bool, Error> {
    let mut scratch = [0; 16];
    Ok(!matches!(
        read_setting(host, key, &mut scratch)?,
        SettingValue::Bool(false)
    ))
}

fn twelve_hours(host: &Host<'_>) -> Result<bool, Error> {
    let mut scratch = [0; 32];
    Ok(matches!(
        read_setting(host, "core.clock-format", &mut scratch)?,
        SettingValue::Text(text) if text == paperpaw_sdk::time::CLOCK_12H
    ))
}

fn minute_of(time: LocalTime) -> u16 {
    u16::from(time.hour) * 60 + u16::from(time.minute)
}

/// The area a part takes, from the top left of the screen's part of the
/// display, `reach` pixels larger on every side.
fn area_of(origin: (i32, i32), rect: Rect, reach: i32) -> FrameRect {
    let clamp = paperpaw_ui_kit::geometry::clamp_u16;
    let x = i32::from(rect.x) - origin.0 - reach;
    let y = i32::from(rect.y) - origin.1 - reach;
    paint::frame_rect(Rect::new(
        clamp(i64::from(x.max(0))),
        clamp(i64::from(y.max(0))),
        clamp(i64::from(rect.width) + 2 * i64::from(reach)),
        clamp(i64::from(rect.height) + 2 * i64::from(reach)),
    ))
}
