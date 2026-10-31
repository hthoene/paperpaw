//! A plugin's row in the store's lists: its name and its state on the
//! first line (a mark where it is installed or has an update, its size
//! otherwise), what it does on one line below.

use alloc::format;
use alloc::string::{String, ToString};

use paperpaw_sdk::install::{Origins, Progress};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::{Color, Env, Rect, files};

use crate::catalog::Entry;

/// The height of a plugin's row: the title line and one line of small
/// text.
pub(crate) fn height(style: &UiStyle) -> u32 {
    style.noted_row_of(1)
}

/// What a plugin is to this device, for its mark.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum State {
    /// Not here: its size.
    Offered(String),
    Installed,
    Update,
    /// Downloading, or waiting for the reader or a restart: said in words.
    Busy(String),
}

/// The state of `plugin`: how far its download is, whether it waits for
/// the reader, its turn in the queue (`queued`) or a restart, whether it
/// is here (and older), or its size.
pub(crate) fn state(
    env: &Env<'_>,
    job: Option<&Progress>,
    queued: &[String],
    plugin: &Entry,
    origins: &Origins,
) -> State {
    if let Some(job) = job.filter(|j| j.plugin == plugin.id) {
        if job.done >= job.total {
            return State::Busy(env.i18n().tr("store.state.waiting").to_string());
        }
        return State::Busy(format!("{} / {}", job.done, job.total));
    }
    if queued.contains(&plugin.id) {
        return State::Busy(env.i18n().tr("store.state.queued").to_string());
    }
    let restart = env
        .plugins()
        .get(&plugin.id)
        .is_some_and(paperpaw_sdk::PluginInfo::restart_needed);
    if restart {
        State::Busy(env.i18n().tr("store.state.restart").to_string())
    } else if plugin.newer_in(env, origins) {
        State::Update
    } else if plugin.installed(env).is_some() {
        State::Installed
    } else {
        State::Offered(files::size(plugin.size, env.i18n().tr("store.decimal")))
    }
}

/// Draws the row of `plugin` in `rect`.
pub(crate) fn draw(
    ui: &mut ui::Ui<'_>,
    rect: Rect,
    focused: bool,
    plugin: &Entry,
    job: Option<&Progress>,
    queued: &[String],
    origins: &Origins,
) {
    let (env, style) = (ui.env(), ui.style());
    let Some(font) = ui.font() else {
        return;
    };
    let band = ui::title_area(style, rect);
    let margin = style.margin() as f32;
    let right = f32::from(rect.x) + f32::from(rect.width) - margin;
    let ink = style.focus_ink(focused);
    // The mark first, from the right margin; the name gets the rest.
    let mark_left = match state(env, job, queued, plugin, origins) {
        State::Offered(text) | State::Busy(text) => {
            let width = font.measure(style.small(), &text);
            let x = right - width;
            ui::Label::new(&text)
                .px(style.small())
                .color(style.focus_faint(focused, ui::Ink::DETAIL))
                .draw_from(ui, x, band);
            x
        }
        State::Installed => {
            let text = env.i18n().tr("store.state.installed");
            let width = font.measure(style.small(), text);
            let icon = style.small() * ui::ICON_SCALE;
            let icon_width = ui::icon_width(env, "check", icon);
            let x = right - width;
            ui::Label::new(text)
                .px(style.small())
                .color(ink)
                .draw_from(ui, x, band);
            let icon_x = x - icon_width - style.gap() as f32 / 2.0;
            ui.icon("check", icon, icon_x, band, ink);
            icon_x
        }
        State::Update => {
            let text = env.i18n().tr("store.state.update");
            draw_pill(ui, band, right, text, focused)
        }
    };
    let name_area = Rect::new(
        band.x,
        band.y,
        (mark_left - style.gap() as f32 - f32::from(band.x)).max(0.0) as u16,
        band.height,
    );
    ui::Label::new(&plugin.name)
        .px(style.text())
        .color(ink)
        .draw_from(ui, f32::from(rect.x) + margin, name_area);
    // What it does: one line, shortened where longer; PaperPaw's own
    // say so first.
    let summary = if plugin.official {
        format!("{} · {}", env.i18n().tr("store.official"), plugin.summary)
    } else if plugin.unreviewed {
        format!("{} · {}", env.i18n().tr("store.unreviewed"), plugin.summary)
    } else {
        plugin.summary.clone()
    };
    let below = band.y + band.height;
    let line = (style.small() * 1.3) as u16;
    let note = Rect::new(rect.x, below, (right - f32::from(rect.x)) as u16, line);
    ui::Label::new(&summary)
        .px(style.small())
        .color(style.focus_faint(focused, ui::Ink::DETAIL))
        .draw_from(ui, f32::from(rect.x) + margin, note);
}

/// Draws `text` as a row's note: one line of small text below its title,
/// from `left` (where its title begins, after any icon).
pub(crate) fn draw_note(ui: &mut ui::Ui<'_>, rect: Rect, left: u16, focused: bool, text: &str) {
    let style = ui.style();
    let band = ui::title_area(style, rect);
    let margin = style.margin();
    let line = (style.small() * 1.3) as u16;
    let right = rect.x + rect.width - margin;
    let area = Rect::new(left, band.y + band.height, right.saturating_sub(left), line);
    let x = f32::from(left + margin);
    let color = style.focus_faint(focused, ui::Ink::DETAIL);
    ui::Label::new(text)
        .px(style.small())
        .color(color)
        .draw_from(ui, x, area);
}

/// Draws `text` as a small filled label ending at `right`, centred on
/// `band`: what wants doing stands out. Returns where it begins.
pub(crate) fn draw_pill(
    ui: &mut ui::Ui<'_>,
    band: Rect,
    right: f32,
    text: &str,
    focused: bool,
) -> f32 {
    let style = ui.style();
    let Some(font) = ui.font() else {
        return right;
    };
    let pad = style.gap() as f32;
    let width = font.measure(style.small(), text) + 2.0 * pad;
    let height = font.line_metrics(style.small()).line_height + pad / 2.0;
    let x = right - width;
    let y = f32::from(band.y) + (f32::from(band.height) - height) / 2.0;
    let pill = Rect::new(x as u16, y as u16, width as u16, height as u16);
    let (paper, letters) = if focused {
        (Color::WHITE, Color::BLACK)
    } else {
        (Color::BLACK, Color::WHITE)
    };
    ui.canvas()
        .fill_round(pill, u32::from(pill.height) / 2, paper);
    ui::Label::new(text)
        .px(style.small())
        .color(letters)
        .draw_from(ui, x + pad, pill);
    x
}
