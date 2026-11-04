//! What can be done with a book: a sheet from the bottom with its cover,
//! title, author and how far it is read, then the actions (go on reading
//! or open, its series, finished or unread, favourite, information), and
//! those plugins add to the book menu ([`menus::BOOK`]); last, deleting
//! it, which the sheet asks about in place (the book still in sight, the
//! focus on keeping it) before the library deletes it
//! ([`library::Library::delete`]).
//!
//! The book's details and small cover are read when the sheet opens.
//! What changes goes through the library service, which tells every view
//! ([`library::service`]).

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, events, menus, params, slots};
use paperpaw_sdk::library::{self, catalog};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets;
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect,
    RefreshMode, Screen, StorageError,
};

use super::rows::{self, Loaded, Sizes};
use super::shelf::{self, Kind};

/// An action of the sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Open,
    Series,
    Finished,
    Unfinished,
    Favourite,
    Unfavourite,
    Info,
    Archive,
    /// Plugins' action `n` of the book menu.
    Plugin(usize),
    /// Deleting: asks first.
    Delete,
    Menu,
}

/// The answers when deleting is asked about: their text, and whether
/// they delete.
const ANSWERS: [(&str, bool); 2] = [("files.delete.yes", true), ("files.delete.keep", false)];

pub(crate) struct BookSheet {
    style: UiStyle,
    sizes: Sizes,
    region: Rect,
    /// The part with the cover, title, author and progress.
    head: Rect,
    list: ListView,
    /// Below the actions, where more come than the sheet holds.
    pager: Option<ui::Pager>,
    book: Loaded,
    actions: Vec<(Action, String)>,
    /// Plugins' actions of the book menu ([`Action::Plugin`]).
    plugins: Vec<Id>,
    /// While deleting is asked about: the answers, at the bottom of the
    /// actions' place, the question above them.
    asking: Option<ListView>,
}

impl BookSheet {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let sizes = Sizes::new(env, &style);
        let path = params.get(params::PATH).unwrap_or_default().to_string();
        let storage = env.storage();
        // Its cover read from the book where the library's walk did not
        // yet (one book: a moment).
        let _ = library::indexed(env, &path);
        let details = library::service(env).book(env, &path).unwrap_or_default();
        let opened = library::opened_books(storage).contains(&path);
        let favourite = library::favourites(storage).contains(&path);
        let status = catalog::Status::new(details.progress, favourite, opened);
        let record = catalog::Record::of(&path, &details).with_status(status);
        let mut book = Loaded::load(env, record, sizes.cover, "");
        let note = book.author.clone();
        let width = env.device().display.width;
        rows::prepare_row(env, &style, &sizes, width, &mut book, note, false);
        let tr = |key: &str| env.i18n().tr(key).to_string();
        let mut actions = Vec::new();
        let open = match book.progress {
            1..=99 => "files.list.continue",
            _ => "files.option.open",
        };
        actions.push((Action::Open, tr(open)));
        if !book.series.is_empty() {
            let label = env.i18n().format(
                "files.list.option.series",
                &[("series", Arg::Str(&book.series))],
            );
            actions.push((Action::Series, label));
        }
        if book.progress == 100 {
            actions.push((Action::Unfinished, tr("files.option.unfinished")));
        } else {
            actions.push((Action::Finished, tr("files.option.finished")));
        }
        if favourite {
            actions.push((Action::Unfavourite, tr("files.option.unfavourite")));
        } else {
            actions.push((Action::Favourite, tr("files.option.favourite")));
        }
        actions.push((Action::Info, tr("files.option.info")));
        if crate::can_archive(env, &path) {
            actions.push((Action::Archive, tr("files.option.archive")));
        }
        let mut plugins = Vec::new();
        // As shown now, with the label and badge their plugins give them
        // (*Open with …* only where there is something to choose).
        for item in env.menus().shown_items(env, menus::BOOK.as_str()) {
            if item.section {
                continue;
            }
            let label = env
                .menus()
                .state(env, item.id.as_str())
                .text(&tr(item.title));
            actions.push((Action::Plugin(plugins.len()), label));
            plugins.push(item.id.clone());
        }
        if library::service(env).can_delete(env, &path) {
            actions.push((Action::Delete, tr("files.option.delete")));
        }
        if !env.device().input.touch {
            actions.push((Action::Menu, tr("files.option.menu")));
        }

        let display = env.device().display;
        let gap = style.gap();
        let head_h = sizes.cover.1 + 2 * gap;
        let row = style.touch_row(u32::from(style.row())) as u16;
        // As high as its actions need, the header of the screen below
        // still in sight; more actions (plugins add to the book menu) go
        // on pages, with a pager below them.
        let most = display.height.saturating_sub(style.header() + head_h + gap);
        let all = row * actions.len() as u16;
        let pager_h = ui::Pager::height(&style);
        let (rows_h, paged) = if all <= most {
            (all, false)
        } else {
            (most.saturating_sub(pager_h) / row.max(1) * row, true)
        };
        let height = head_h + rows_h + if paged { pager_h } else { 0 } + gap;
        let region = Rect::new(0, display.height - height, display.width, height);
        let head = Rect::new(0, region.y + gap / 2, display.width, head_h);
        let list = ListView::new(
            Rect::new(0, head.y + head.height, display.width, rows_h),
            u32::from(row),
        )
        .focus_on_keys(env);
        let pager = paged.then(|| {
            let area = Rect::new(0, head.y + head.height + rows_h, display.width, pager_h);
            ui::Pager::new(env, area)
        });
        Self {
            style,
            sizes,
            region,
            head,
            list,
            pager,
            book,
            actions,
            plugins,
            asking: None,
        }
    }

    /// Asks whether to delete the book, in place of the actions: the
    /// answers in their last two rows, the focus on keeping it.
    fn ask(&mut self, cx: &mut Cx<'_, '_>) {
        let area = self.list.area();
        // As high as the actions' rows, at the sheet's bottom edge (where
        // the actions end, or their pager, which is not shown meanwhile).
        let row = self.style.touch_row(u32::from(self.style.row())) as u16;
        let r = self.region;
        let bottom = (r.y + r.height).saturating_sub(self.style.gap() / 2);
        let rows = Rect::new(area.x, bottom.saturating_sub(2 * row), area.width, 2 * row);
        let mut answers = ListView::new(rows, u32::from(row)).focus_on_keys(cx.env());
        answers.set_focus(1);
        self.asking = Some(answers);
        cx.request_redraw_area(self.region);
        cx.request_refresh(RefreshMode::Fast);
    }

    /// The answer `index` to asking about deleting.
    fn answer(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((_, delete)) = ANSWERS.get(index) else {
            return;
        };
        if !delete {
            self.asking = None;
            cx.request_redraw_area(self.region);
            cx.request_refresh(RefreshMode::Fast);
            return;
        }
        let env = *cx.env();
        cx.command(Command::Back);
        let deleted = library::service(&env).delete(&env, &[self.book.path.as_str()]);
        // Why not, where it failed.
        let key = if !deleted.refused.is_empty() {
            "files.delete-failed.not-book"
        } else {
            match deleted.failed.first().map(|(_, error)| error) {
                None => "files.deleted",
                Some(StorageError::Unsupported) => "files.delete-failed.read-only",
                Some(_) => "files.delete-failed",
            }
        };
        for command in deleted.commands {
            cx.command(command);
        }
        cx.command(Command::Notify(env.i18n().tr(key).to_string()));
    }

    fn choose(&mut self, index: usize, cx: &mut Cx<'_, '_>) {
        let Some((action, _)) = self.actions.get(index) else {
            return;
        };
        if *action == Action::Delete {
            return self.ask(cx);
        }
        let env = *cx.env();
        let path = self.book.path.as_str();
        cx.command(Command::Back);
        match action {
            Action::Open => super::read(path, cx),
            Action::Series => cx.command(shelf::open(Kind::Series, &self.book.series)),
            Action::Finished | Action::Unfinished => {
                let finished = *action == Action::Finished;
                cx.command(library::service(&env).set_finished(&env, path, finished));
            }
            Action::Favourite | Action::Unfavourite => {
                let favourite = *action == Action::Favourite;
                cx.command(library::service(&env).set_favourite(&env, path, favourite));
                let key = if favourite {
                    "files.favourite-added"
                } else {
                    "files.favourite-removed"
                };
                cx.command(Command::Notify(env.i18n().tr(key).to_string()));
            }
            Action::Info => cx.command(Command::OpenSlot(
                slots::BOOK_INFO,
                Params::new().with(params::PATH, path.to_string()),
            )),
            Action::Archive => crate::archive(&env, path, cx),
            Action::Plugin(n) => {
                let Some(id) = self.plugins.get(*n) else {
                    return;
                };
                // Which book, then the action as usual.
                let chosen = events::BookChosen::new(id.clone(), path.to_string());
                cx.command(events::BOOK_CHOSEN.emit(chosen));
                cx.command(Command::run(id.clone()));
            }
            Action::Menu => super::open_menu(cx),
            Action::Delete => {}
        }
    }
}

impl Screen for BookSheet {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let r = self.region;
        let mut ui = ui::Ui::new(canvas, env, &style);
        ui.canvas().fill(r, Color::WHITE);
        ui.canvas().fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            (u32::from(style.line()) * 2).max(2),
            Color::BLACK,
        );
        // The book as a row of the list, but larger text: its cover, title,
        // author and state, never focused (so told as a heading, not a row).
        let heading = paperpaw_sdk::semantics::Role::Heading;
        rows::draw_book(
            &mut ui,
            &self.sizes,
            self.head,
            false,
            &self.book,
            false,
            heading,
        );
        ui.separator(self.head, 0);
        if let Some(answers) = &self.asking {
            // The question right below the book, above the answers, as
            // long as the room allows (smaller where it needs more).
            let area = self.list.area();
            let (margin, gap) = (style.margin(), style.gap());
            let width = area.width.saturating_sub(2 * margin);
            let room = answers.area().y.saturating_sub(area.y + 2 * gap);
            let text = env.i18n().tr("files.delete.question");
            let (most, least) = (style.text(), style.small());
            let question = widgets::Wrapped::fitting(env, text, most, least, width, room);
            // Told as the heading over the answers, across its room (not
            // as a text, which a row of the list below would take in).
            let heading = paperpaw_sdk::semantics::Role::Heading;
            let room_rect = Rect::new(margin, area.y, width, room + 2 * gap);
            ui.canvas().describe(heading, text, room_rect);
            let at = Rect::new(margin, area.y + gap, width, question.height(env).min(room));
            question.draw(ui.canvas(), env, at, widgets::Align::Left, Color::BLACK);
            answers.draw(&mut ui, ANSWERS.len(), |ui, index, rect, focused| {
                let label = env.i18n().tr(ANSWERS[index].0);
                ui::Row::plain(label).focused(focused).draw(ui, rect);
            });
            return;
        }
        let actions = &self.actions;
        self.list
            .draw(&mut ui, actions.len(), |ui, index, rect, focused| {
                let label = &actions[index].1;
                ui::Row::plain(label).focused(focused).draw(ui, rect);
            });
        if let (Some(pager), Some(page)) = (&self.pager, self.list.paging(actions.len())) {
            pager.draw(&mut ui, page);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        let a = action;
        if let Some(answers) = &mut self.asking {
            // Back keeps the book, as the answer does.
            if *a == actions::BACK {
                self.asking = None;
                cx.request_redraw_area(self.region);
                cx.request_refresh(RefreshMode::Fast);
                return Handled::Yes;
            }
            if *a == actions::CONFIRM {
                let focus = answers.focus();
                self.answer(focus, cx);
                return Handled::Yes;
            }
            let back = [
                actions::UP,
                actions::LEFT,
                actions::PREVIOUS,
                actions::PREV_PAGE,
            ];
            let on = [
                actions::DOWN,
                actions::RIGHT,
                actions::NEXT,
                actions::NEXT_PAGE,
            ];
            let delta = if back.contains(a) {
                -1
            } else if on.contains(a) {
                1
            } else {
                return Handled::No;
            };
            answers
                .move_focus_in(cx.env(), delta, ANSWERS.len())
                .request(cx);
            return Handled::Yes;
        }
        let delta = if [
            actions::UP,
            actions::LEFT,
            actions::PREVIOUS,
            actions::PREV_PAGE,
        ]
        .contains(a)
        {
            -1
        } else if [
            actions::DOWN,
            actions::RIGHT,
            actions::NEXT,
            actions::NEXT_PAGE,
        ]
        .contains(a)
        {
            1
        } else if *a == actions::CONFIRM {
            self.choose(self.list.focus(), cx);
            return Handled::Yes;
        } else {
            return Handled::No;
        };
        self.list
            .move_focus_in(cx.env(), delta, self.actions.len())
            .request(cx);
        Handled::Yes
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(answers) = &self.asking {
            if let Some(index) = answers.hit(gesture.start, ANSWERS.len()) {
                self.answer(index, cx);
            }
            return Handled::Yes;
        }
        let len = self.actions.len();
        if let Some(action) = self.pager.and_then(|pager| pager.tap(gesture.start)) {
            // The first entry of the page before or after.
            let rows = self.list.rows();
            let page = self.list.page();
            let to = if action == actions::NEXT_PAGE {
                ((page + 1) * rows).min(len.saturating_sub(1))
            } else {
                page.saturating_sub(1) * rows
            };
            let change = self.list.set_focus(to);
            change.request(cx);
            return Handled::Yes;
        }
        if let Some(index) = self.list.hit(gesture.start, len) {
            self.choose(index, cx);
        } else if self.head.contains(gesture.start) {
            // The book itself: open it.
            self.choose(0, cx);
        }
        Handled::Yes
    }
}
