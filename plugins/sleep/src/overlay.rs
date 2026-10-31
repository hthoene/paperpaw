//! *Page with a picture*: the page read stays, a picture over it, its
//! clear parts showing the page (a PNG's or GIF's transparency) and all
//! of it as see-through as the reader likes.
//!
//! The picture comes from the pictures folder (one after the other, as
//! for *My pictures*), from a file a preset names, or is a widget (a
//! clock, the day's reading) on a panel of its own: any plugin, also a
//! data plugin's `presets.ini`, offers pictures as presets of kind
//! [`KIND`] (`sleep.overlay-picture = plugins/owls/owl.png`, with its
//! place and size if it likes). Settings: where it goes, how large it
//! is, how much the page shows through, and when it is used (always,
//! while reading, or elsewhere; else the book's cover shows). Where a
//! screen guards the device (a lock), the page is not to be seen: the
//! cover shows.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, slots};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, ListView, UiStyle};
use paperpaw_sdk::widgets::Widget;
use paperpaw_sdk::{
    Canvas, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, RefreshMode, Screen,
    SettingValue,
};

/// The kind of the presets that offer pictures.
pub const KIND: &str = "sleep.overlay";
/// The picture: a file, or empty for the pictures folder.
pub const PICTURE: Setting<Text> = Setting::new("sleep.overlay-picture");
/// A widget shown in place of the picture, if one is set.
pub const WIDGET: Setting<Text> = Setting::new("sleep.overlay-widget");
/// Where it goes: one of [`POSITIONS`].
pub const POSITION: Setting<Text> = Setting::new("sleep.overlay-position");
pub const POSITIONS: [&str; 7] = [
    "sleep.position.center",
    "sleep.position.top",
    "sleep.position.bottom",
    "sleep.position.top-left",
    "sleep.position.top-right",
    "sleep.position.bottom-left",
    "sleep.position.bottom-right",
];
/// Where it goes unless set: out of the way of the text, below on the
/// right.
pub const DEFAULT_POSITION: &str = POSITIONS[6];
/// How large: percent of the screen (a widget: of its shorter side), so
/// 100 % lets a picture made for the whole screen fill it.
pub const SIZE: Setting<i64> = Setting::new("sleep.overlay-size");
/// How large unless set.
pub const DEFAULT_SIZE: i64 = 35;
/// How much the page shows through the picture, in percent.
pub const SEE_THROUGH: Setting<i64> = Setting::new("sleep.overlay-see-through");
/// When it is used: one of [`WHEN`].
pub const USED: Setting<Text> = Setting::new("sleep.overlay-when");
pub const WHEN: [&str; 3] = [
    "sleep.when.always",
    "sleep.when.reading",
    "sleep.when.elsewhere",
];
/// The screen choosing the picture: the folder's, a preset's, a widget.
pub const PICK: Id = Id::new_static("sleep.pick-overlay");

/// Whether the picture goes over the page now: as [`USED`] says, by
/// whether a book is read on the topmost full screen (with or without a
/// menu over it); never where a screen guards the device.
pub(crate) fn used(env: &Env<'_>) -> bool {
    if env.stack().iter().any(|entry| entry.guards) {
        return false;
    }
    let reading = env
        .stack()
        .iter()
        .rev()
        .find(|entry| !entry.overlay)
        .is_some_and(|entry| entry.slot.as_ref() == Some(&slots::READER));
    match env.settings().text(USED.as_str()) {
        w if w == WHEN[1] => reading,
        w if w == WHEN[2] => !reading,
        _ => true,
    }
}

/// What goes over the page.
pub(crate) enum Over {
    Picture(ImageRef),
    Widget(Box<dyn Widget>),
}

impl core::fmt::Debug for Over {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Over::Picture(image) => f.debug_tuple("Picture").field(image).finish(),
            Over::Widget(_) => f.write_str("Widget"),
        }
    }
}

/// What goes over the page, made for its box; why not, in the reader's
/// language, where nothing can (a file PaperPaw does not read, an empty
/// folder).
pub(crate) fn over(env: &Env<'_>) -> Result<Over, String> {
    let tr = |key: &str, name: &str| env.i18n().tr(key).replace("{name}", name);
    let widget = env.settings().text(WIDGET.as_str()).trim();
    if !widget.is_empty() {
        return env
            .widgets()
            .make(widget, env)
            .map(Over::Widget)
            .ok_or_else(|| tr("sleep.overlay.no-widget", widget));
    }
    let chosen = env.settings().text(PICTURE.as_str()).trim().to_string();
    let path = if chosen.is_empty() {
        crate::next_from_folder(env).ok_or_else(|| tr("sleep.overlay.empty-folder", ""))?
    } else {
        chosen
    };
    picture(env, &path).map(Over::Picture)
}

/// The picture at `path` decoded with its transparency, as large as it
/// may show; the decoded file of the sleep before where it is the same
/// picture at the same size (nothing written to the card then).
fn picture(env: &Env<'_>, path: &str) -> Result<ImageRef, String> {
    let (box_w, box_h) = box_size(env);
    let size = format!("{box_w}x{box_h}");
    let decoded = crate::data("overlay.gray");
    let kept = crate::data("overlay.key");
    let missing = || {
        env.i18n()
            .tr("sleep.overlay.missing")
            .replace("{name}", path)
    };
    let key = key(env, path, &size).ok_or_else(missing)?;
    let same =
        paperpaw_sdk::read_file(env.storage(), &kept, 1024).is_ok_and(|k| k == key.as_bytes());
    if same && let Some(file) = paperpaw_sdk::image::AlphaFile::open(env.storage(), &decoded) {
        return Ok(ImageRef {
            path: file.path,
            width: file.width,
            height: file.height,
        });
    }
    // The key names the decoded file only once it is decoded.
    let _ = env.storage().remove(&kept);
    let image = paperpaw_sdk::image::picture_with_alpha(
        env.storage(),
        path,
        &decoded,
        paperpaw_sdk::image::Fit::screen(u32::from(box_w), u32::from(box_h)),
    )
    .map_err(|e| {
        env.i18n()
            .tr("sleep.overlay.unreadable")
            .replace("{name}", path)
            .replace("{reason}", &format!("{e}"))
    })?;
    let _ = env.storage().write(&kept, key.as_bytes());
    Ok(image)
}

/// What tells the picture at `path` made `size` large from others: its
/// path, size, the size it is made and a fingerprint of its first and
/// last bytes (the card keeps no times of change PaperPaw reads).
fn key(env: &Env<'_>, path: &str, size_made: &str) -> Option<String> {
    let size = env.storage().size(path).ok()?;
    let mut hash: u32 = 0x811c_9dc5;
    let mut buf = [0u8; 512];
    for at in [0, size.saturating_sub(buf.len() as u64)] {
        let n = env.storage().read_at(path, at, &mut buf).ok()?;
        for &b in &buf[..n] {
            hash = (hash ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
    }
    Some(format!("{path}\n{size}\n{size_made}\n{hash:08x}\n"))
}

/// The size setting, in percent.
fn percent(env: &Env<'_>) -> u32 {
    env.settings().read(&SIZE).clamp(10, 100) as u32
}

/// The side of the square a widget fits in.
fn box_side(env: &Env<'_>) -> u16 {
    let display = env.device().display;
    (u32::from(display.width.min(display.height)) * percent(env) / 100) as u16
}

/// The box a picture fits in: the screen, so much of it as the size says;
/// all of it at 100 %, which no margin or status line narrows.
fn box_size(env: &Env<'_>) -> (u16, u16) {
    let display = env.device().display;
    let percent = percent(env);
    let part = |side: u16| (u32::from(side) * percent / 100) as u16;
    (part(display.width), part(display.height))
}

/// Where something `w` × `h` goes on the screen: inside the margins and
/// clear of the status lines above and below (the reader's, the time and
/// battery), as [`POSITION`] says.
fn placed(env: &Env<'_>, w: u16, h: u16, fill: bool) -> Rect {
    let display = env.device().display;
    let style = UiStyle::new(env);
    // A picture at 100 % has the whole screen, to the edges.
    let (margin, status) = match fill {
        true => (0, 0),
        false => (
            style.margin(),
            paperpaw_sdk::widgets::line_height(env, style.small()) + style.gap(),
        ),
    };
    let (w, h) = (
        w.min(display.width.saturating_sub(2 * margin)),
        h.min(display.height.saturating_sub(2 * (margin + status))),
    );
    let (left, right) = (margin, display.width.saturating_sub(w + margin));
    let (top, bottom) = (
        margin + status,
        display.height.saturating_sub(h + margin + status),
    );
    let (mid_x, mid_y) = (
        display.width.saturating_sub(w) / 2,
        display.height.saturating_sub(h) / 2,
    );
    let position = env.settings().text(POSITION.as_str());
    let (x, y) = match POSITIONS.iter().position(|p| *p == position) {
        Some(0) => (mid_x, mid_y),
        Some(1) => (mid_x, top),
        Some(2) => (mid_x, bottom),
        Some(3) => (left, top),
        Some(4) => (right, top),
        Some(5) => (left, bottom),
        _ => (right, bottom),
    };
    Rect::new(x, y, w, h)
}

/// The padding of a widget's panel.
fn pad(env: &Env<'_>) -> u16 {
    UiStyle::new(env).gap() * 2
}

/// `area` less `pad` all round.
fn inside(area: Rect, pad: u16) -> Rect {
    Rect::new(
        area.x + pad,
        area.y + pad,
        area.width.saturating_sub(2 * pad),
        area.height.saturating_sub(2 * pad),
    )
}

/// Where `over` goes on the screen; a widget is placed there.
pub(crate) fn place(env: &Env<'_>, over: &mut Over) -> Rect {
    let side = box_side(env);
    match over {
        Over::Picture(image) => {
            let (box_w, box_h) = box_size(env);
            let (w, h) = ui::fit_image(image, box_w, box_h);
            placed(env, w, h, percent(env) == 100)
        }
        Over::Widget(widget) => {
            let pad = pad(env);
            let height = widget.height(env, side.saturating_sub(2 * pad)) + 2 * pad;
            let area = placed(env, side, height, false);
            widget.place(env, inside(area, pad));
            area
        }
    }
}

/// Draws `over` in `area`, `opacity` (0 to 255) letting the page show
/// through: a picture as it is, a widget on a panel of paper, the page
/// showing through that.
pub(crate) fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, over: &Over, area: Rect, opacity: u8) {
    match over {
        Over::Picture(image) => ui::draw_image_over(canvas, env.storage(), image, area, opacity),
        Over::Widget(widget) => {
            let paper: Vec<u8> = [255u8, 255].repeat(usize::from(area.width));
            for y in area.y..area.bottom() as u16 {
                canvas.draw_gray_alpha_row(i32::from(area.x), i32::from(y), &paper, opacity);
            }
            widget.draw(canvas, env, inside(area, pad(env)));
        }
    }
}

/// How opaque the picture is drawn (0 to 255).
pub(crate) fn opacity(env: &Env<'_>) -> u8 {
    let through = env.settings().read(&SEE_THROUGH).clamp(0, 100) as u32;
    (255 * (100 - through) / 100) as u8
}

/// Why the picture did not show at the last sleep, kept for the picker
/// (nothing is said while sleeping); `None` clears it. Written only when
/// it changed.
pub(crate) fn remember_failure(env: &Env<'_>, why: Option<&str>) {
    let file = crate::data("overlay-failed.txt");
    let before = paperpaw_sdk::read_file(env.storage(), &file, 1024).ok();
    match why {
        Some(why) if before.as_deref() != Some(why.as_bytes()) => {
            let _ = env.storage().write(&file, why.as_bytes());
        }
        None if before.is_some() => {
            let _ = env.storage().remove(&file);
        }
        _ => {}
    }
}

/// A row of the picker.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Source {
    Folder,
    Preset(Id),
    Widget(String),
}

/// Chooses what goes over the page: the pictures folder, one of the
/// presets of kind [`KIND`] (applied as presets are), or a widget. Why
/// the picture did not show at the last sleep is noted on the chosen row.
pub(crate) struct Picker {
    rows: Vec<(Source, String)>,
    /// Why the last sleep showed no picture, if it did not, and where
    /// that is said.
    failed: Option<(String, Rect)>,
    style: UiStyle,
    header: ui::Header,
    list: ListView,
}

impl Picker {
    pub(crate) fn new(env: &Env<'_>, _: &Params) -> Self {
        let mut rows = Vec::from([(
            Source::Folder,
            env.i18n().tr("sleep.overlay.folder").to_string(),
        )]);
        rows.extend(env.settings().presets().of_kind(KIND).map(|p| {
            (
                Source::Preset(p.id.clone()),
                env.i18n().tr(&p.title).to_string(),
            )
        }));
        let widget = env.i18n().tr("sleep.overlay.widget");
        rows.extend(env.widgets().iter().map(|(id, title)| {
            let name = env.i18n().tr(title);
            (
                Source::Widget(id.to_string()),
                widget.replace("{name}", name),
            )
        }));
        let style = UiStyle::new(env);
        let display = env.device().display;
        let header = ui::Header::new(env, &style, true);
        let top = header.area().height + style.gap();
        let note = style.noted_row() as u16;
        let failed =
            paperpaw_sdk::read_file(env.storage(), &crate::data("overlay-failed.txt"), 1024)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .map(|why| {
                    let text = env
                        .i18n()
                        .tr("sleep.overlay.failed")
                        .replace("{reason}", &why);
                    let area =
                        Rect::new(0, display.height.saturating_sub(note), display.width, note);
                    (text, area)
                });
        let bottom = failed.as_ref().map_or(display.height, |(_, area)| area.y);
        let area = Rect::new(0, top, display.width, bottom.saturating_sub(top));
        Self {
            rows,
            failed,
            list: ListView::new(area, u32::from(style.row())).focus_on_keys(env),
            style,
            header,
        }
    }

    /// Whether row `row` is what is chosen now.
    fn chosen(&self, env: &Env<'_>, row: usize) -> bool {
        let widget = env.settings().text(WIDGET.as_str()).trim();
        match &self.rows[row].0 {
            Source::Widget(id) => widget == id,
            _ if !widget.is_empty() => false,
            Source::Folder => env.settings().text(PICTURE.as_str()).trim().is_empty(),
            Source::Preset(id) => env
                .settings()
                .presets()
                .get(id.as_str())
                .is_some_and(|preset| {
                    let changes = preset.changes(env.settings(), &[]);
                    paperpaw_sdk::presets::applied(env.settings(), &changes)
                }),
        }
    }

    fn pick(&self, row: usize, cx: &mut Cx<'_, '_>) {
        let env = *cx.env();
        let set = |cx: &mut Cx<'_, '_>, setting: &Setting<Text>, value: &str| {
            cx.command(Command::SetSetting(
                setting.id().clone(),
                SettingValue::Text(Cow::Owned(value.to_string())),
            ));
        };
        match self.rows.get(row).map(|r| &r.0) {
            Some(Source::Folder) => {
                set(cx, &WIDGET, "");
                set(cx, &PICTURE, "");
            }
            Some(Source::Widget(id)) => set(cx, &WIDGET, id),
            Some(Source::Preset(id)) => {
                set(cx, &WIDGET, "");
                if let Some(preset) = env.settings().presets().get(id.as_str()) {
                    let changes = preset.changes(env.settings(), &[]);
                    for command in paperpaw_sdk::presets::commands(env.settings(), &changes) {
                        cx.command(command);
                    }
                }
            }
            None => return,
        }
        cx.command(Command::Back);
    }
}

impl Screen for Picker {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        let len = self.rows.len();
        let title = env.i18n().tr("sleep.setting.overlay-picture");
        self.header
            .draw(&mut ui, title, None, self.list.paging(len));
        let on = env.i18n().tr("core.state.on");
        let chosen: Vec<bool> = (0..len).map(|row| self.chosen(env, row)).collect();
        let rows = &self.rows;
        self.list.draw(&mut ui, len, |ui, row, rect, focused| {
            let value = if chosen[row] { on } else { "" };
            ui::Row::value(&rows[row].1, value)
                .focused(focused)
                .draw(ui, rect);
        });
        if let Some((text, area)) = &self.failed {
            ui.note_line(*area, text);
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if *action == actions::CONFIRM {
            match self.list.confirm() {
                Some(row) => self.pick(row, cx),
                None => self.list.focus_change().request(cx),
            }
            return Handled::Yes;
        }
        match self.list.navigate(action, self.rows.len()) {
            Some(change) => {
                change.request(cx);
                Handled::Yes
            }
            None => Handled::No,
        }
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        let paging = self.list.paging(self.rows.len());
        if let Some(action) = self
            .header
            .tap(cx.env(), &self.style, gesture.start, paging)
        {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        match self.list.hit(gesture.start, self.rows.len()) {
            Some(row) => {
                self.list.set_focus(row);
                self.pick(row, cx);
                cx.request_refresh(RefreshMode::Fast);
                Handled::Yes
            }
            None => Handled::No,
        }
    }
}

impl core::fmt::Debug for Picker {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Picker")
            .field("rows", &self.rows.len())
            .finish_non_exhaustive()
    }
}
