//! The host's choice dialog (slot `core.choice`): the options of a question
//! (`docs/abi/ui.md` 3.1, kind 1) as a sheet, answered through the request
//! store with the line chosen (outcome 0, `choice` its index). Made with no
//! request (an interface 6 caller, until its cleanup) it replies with an event
//! as the SDK screen did. *Back*, or a
//! close with no answer, cancels (the stack's close tells the store).
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::ui::{self, ListChange, ListView, UiStyle};
use paperpaw_sdk::{
    Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode,
    Screen,
};

/// The options, separated by commas.
pub(super) const OPTIONS: &str = "options";
/// Translation keys of the options: this, then the option.
pub(super) const LABELS: &str = "labels";
/// Lines of the title, at most.
const TITLE_LINES: usize = 3;

/// What option `option` says: the translation of `labels` followed by
/// it, or, without labels, an action's title where it names one (options
/// that are actions, such as the other ways to open a book).
fn label(env: &Env<'_>, labels: &str, option: &str) -> String {
    if labels.is_empty()
        && let Some(action) = env.actions().get(option)
    {
        return env.i18n().tr(action.title()).to_string();
    }
    env.i18n()
        .tr(&alloc::format!("{labels}{option}"))
        .to_string()
}

/// The choice overlay; see the [module documentation](self).
pub(crate) struct Choice {
    style: UiStyle,
    region: Rect,
    title: Rect,
    list: ListView,
    /// Below the options when they take more than one page.
    pager: Option<ui::Pager>,
    title_text: String,
    /// Where the answer goes; none for a dialog made with no request.
    answer: Option<super::Answerer>,
    /// With no request (an interface 6 caller, until its cleanup): the
    /// context and the event the choice is replied with.
    path: String,
    reply: Option<Id>,
    labels: String,
    options: Vec<String>,
}

impl Choice {
    pub(crate) fn new(env: &Env<'_>, params: &Params, answer: Option<super::Answerer>) -> Self {
        let style = UiStyle::new(env);
        let display = env.device().display;
        // A sheet in the readable column where the screen is wider than that.
        let column = ui::readable(env, &style, Rect::new(0, 0, display.width, display.height));
        let options: Vec<String> = params
            .get(OPTIONS)
            .unwrap_or_default()
            .split(',')
            .filter(|o| !o.is_empty())
            .map(ToString::to_string)
            .collect();
        let title_text = params.get(params::TITLE).unwrap_or_default().to_string();
        let row = style.row() as u16;
        // A long title takes up to three lines.
        let lines = style.font(env).map_or((1, 0.0), |font| {
            let width = f32::from(column.width) - 2.0 * style.margin() as f32;
            let count = font
                .wrap(style.title(), &title_text, width)
                .len()
                .clamp(1, TITLE_LINES);
            // Its lines a heading's apart.
            let line = font.line_metrics(style.title()).line_height;
            let apart = font.heading_line_height(style.title());
            (count, line + apart * (count - 1) as f32)
        });
        let title_height = match lines {
            (1, _) => row,
            (_, tall) => tall as u16 + style.gap() as u16,
        };
        // As many options as the screen holds below the title; more go on
        // pages, with a pager below them.
        let row = u32::from(row.max(1));
        let room = u32::from(display.height)
            .saturating_sub(u32::from(title_height) + u32::from(style.gap()) * 2);
        let count = options.len() as u32;
        let (shown, pager_height) = if count * row <= room {
            (count, 0)
        } else {
            let pager = u32::from(ui::Pager::height(&style));
            ((room.saturating_sub(pager) / row).max(1), pager)
        };
        let list_height = shown * row;
        let height =
            (u32::from(title_height) + list_height + pager_height + u32::from(style.gap()) * 2)
                .min(u32::from(display.height)) as u16;
        let region = Rect::new(column.x, display.height - height, column.width, height);
        let title = Rect::new(
            column.x,
            region.y + style.gap() as u16,
            column.width,
            title_height,
        );
        let list_area = Rect::new(
            column.x,
            title.y + title_height,
            column.width,
            list_height as u16,
        );
        let list = ListView::new(list_area, row).focus_on_keys(env);
        let pager = (pager_height > 0).then(|| {
            let area = Rect::new(
                column.x,
                list_area.y + list_area.height,
                column.width,
                pager_height as u16,
            );
            ui::Pager::new(env, area)
        });
        Self {
            style,
            region,
            title,
            list,
            pager,
            title_text,
            answer,
            path: params.get(params::PATH).unwrap_or_default().to_string(),
            reply: params
                .get(params::REPLY)
                .and_then(|reply| Id::parse(reply).ok()),
            labels: params.get(LABELS).unwrap_or_default().to_string(),
            options,
        }
    }

    /// Draws the options again after the focus or the page changed.
    fn moved(&self, change: ListChange, cx: &mut Cx<'_, '_>) {
        match change {
            ListChange::Focus { .. } => change.request(cx),
            ListChange::Page => {
                cx.request_redraw_area(self.region);
                cx.request_refresh(RefreshMode::Fast);
            }
            // Nothing moved (and any change the SDK adds): nothing to draw.
            _ => {}
        }
    }

    fn choose(&self, index: usize, cx: &mut Cx<'_, '_>) {
        // An index the list does not hold is a cancel, never a choice.
        let answer = u32::try_from(index)
            .ok()
            .filter(|_| index < self.options.len())
            .map_or((1, 0), |index| (0, index));
        if let Some(to) = &self.answer {
            to.answer(answer.0, answer.1);
        }
        cx.command(Command::Back);
        if self.answer.is_none()
            && let (Some(option), Some(reply)) = (self.options.get(index), &self.reply)
        {
            cx.command(Command::Emit(
                reply.clone(),
                Params::new()
                    .with(params::PATH, self.path.clone())
                    .with("option", option.clone()),
            ));
        }
    }
}

impl Screen for Choice {
    fn region(&self, _: &Env<'_>) -> Option<Rect> {
        Some(self.region)
    }

    fn dims_below(&self) -> bool {
        true
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let r = self.region;
        ui.canvas().fill(r, Color::WHITE);
        ui.canvas().fill_rect(
            i32::from(r.x),
            i32::from(r.y),
            u32::from(r.width),
            (u32::from(style.line()) * 2).max(2),
            Color::BLACK,
        );
        if let Some(font) = ui.font() {
            let margin = style.margin() as f32;
            let width = f32::from(self.title.width) - 2.0 * margin;
            let from = f32::from(self.title.x) + margin;
            let title = ui::Label::new(&self.title_text)
                .px(style.title())
                .color(Color::BLACK);
            let mut lines = font.wrap(style.title(), &self.title_text, width);
            if lines.len() <= 1 {
                title.draw_from(&mut ui, from, self.title);
            } else {
                let more = lines.len() > TITLE_LINES;
                lines.truncate(TITLE_LINES);
                let line = font.line_metrics(style.title()).line_height;
                let apart = font.heading_line_height(style.title());
                let count = lines.len();
                for (n, range) in lines.into_iter().enumerate() {
                    let text = if more && n + 1 == count {
                        // The rest, shortened.
                        alloc::borrow::Cow::Owned(ui::fit(
                            &font,
                            style.title(),
                            &self.title_text[range.start..],
                            width,
                        ))
                    } else {
                        alloc::borrow::Cow::Borrowed(&self.title_text[range])
                    };
                    let y = self.title.y + (n as f32 * apart) as u16;
                    let area = Rect::new(self.title.x, y, self.title.width, line as u16);
                    ui::Label::new(&text)
                        .px(style.title())
                        .color(Color::BLACK)
                        .draw_from(&mut ui, from, area);
                }
            }
        }
        let (options, labels) = (&self.options, &self.labels);
        self.list
            .draw(&mut ui, options.len(), |ui, index, rect, focused| {
                let label = label(env, labels, &options[index]);
                ui::Row::plain(&label).focused(focused).draw(ui, rect);
            });
        if let Some(pager) = &self.pager {
            let page = ui::Paging::new(self.list.page(), self.list.pages(options.len()));
            pager.draw(&mut ui, page);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            // Nothing to choose: closed at once.
            if self.options.is_empty() {
                self.choose(0, cx);
                return Handled::Yes;
            }
            match self.list.confirm() {
                Some(index) => self.choose(index, cx),
                None => self.moved(self.list.focus_change(), cx),
            }
            return Handled::Yes;
        }
        // While the focus shows (buttons are used, always on the X4, whose
        // buttons beside the screen are the page buttons) the page buttons
        // step a row as the others do; a swipe on a touch screen, the
        // focus hidden, turns a page.
        let page = if self.list.shown() {
            1
        } else {
            self.list.rows() as isize
        };
        match ui::list_step(action, page) {
            Some(delta) => {
                let change = self.list.move_focus_in(cx.env(), delta, self.options.len());
                self.moved(change, cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(index) = self.list.hit(gesture.start, self.options.len()) {
            self.choose(index, cx);
        } else if let Some(action) = self.pager.and_then(|p| p.tap(gesture.start)) {
            // A page on, or back: where its first option is.
            let rows = self.list.rows();
            let page = self.list.page();
            let to = if action == actions::NEXT_PAGE {
                (page + 1) * rows
            } else {
                page.saturating_sub(1) * rows
            };
            if to < self.options.len() && to / rows != page {
                self.list.set_focus(to);
                self.moved(ListChange::Page, cx);
            }
        }
        Handled::Yes
    }
}
