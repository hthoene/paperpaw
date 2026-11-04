//! A picture frame: a picture from a folder on the card (JPEG, PNG,
//! GIF), another each day or each time it shows (the showings counted
//! in memory, see [`Showings`]), the whole of it or its place filled; its
//! name below, if wanted. A tap shows the next one.
//!
//! A picture is decoded once, when it is chosen, into a file of its
//! pixels as large as the screen (as the sleep screen does), read row by
//! row from there each time it is drawn: never whole in memory.

use alloc::boxed::Box;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::image::{ColorFile, Fit, GrayFile, ImageFile};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, UiStyle};
use paperpaw_sdk::widgets::{self, Align, Frame, Padding, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{Arg, Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec, Storage};

use super::quote::CHANGES;
use super::{Changes, Turns, data, of_the_day, today};
use crate::showings::Showings;

pub(super) const ID: Id = Id::new_static("widgets.picture");
/// The folder of the pictures.
const FOLDER: Setting<Text> = Setting::new("widgets.picture.folder");
/// When another picture shows: one of the quote's [`CHANGES`].
const CHANGE: Setting<Text> = Setting::new("widgets.picture.change");
/// How the picture takes its place: one of [`FITS`].
const FIT: Setting<Text> = Setting::new("widgets.picture.fit");
/// Filled, the edges cut where the proportions differ; or whole.
const FITS: [&str; 2] = ["widgets.picture.fit.fill", "widgets.picture.fit.whole"];
/// Its name below it.
const CAPTION: &str = "caption";
/// The next picture, asked for with a tap.
const NEXT: Event<()> = Event::new("widgets.picture.next");
/// The endings of the pictures shown.
const PICTURES: [&str; 4] = [".jpg", ".jpeg", ".png", ".gif"];

/// Where the turns of the pictures are kept (see [`Turns`]).
fn turns_file() -> String {
    data("picture.txt")
}

/// Where the picture shown is decoded to, and what it was decoded from.
fn pixels_file() -> String {
    data("picture.pixels")
}
fn stamp_file() -> String {
    data("picture.stamp")
}

/// The picture's settings kept in the program (its folder is not: it
/// has an editor).
/// The folder of the pictures, chosen on the folder chooser.
static CHOSEN_FOLDER: paperpaw_sdk::SettingExtra =
    paperpaw_sdk::SettingExtra::new().editor_slot(paperpaw_sdk::builtin::slots::CHOOSE_FOLDER);
static FOLDER_SETTING: [SettingSpec; 1] = [SettingSpec::text(
    FOLDER.static_str(),
    "widgets.picture.setting.folder",
    "Pictures",
    ID,
)
.with_extra(&CHOSEN_FOLDER)
.description("widgets.picture.setting.folder.help")];

static SETTINGS: [SettingSpec; 11] = [
    SettingSpec::choice(
        "widgets.picture.change",
        "widgets.setting.change",
        &CHANGES,
        CHANGES[0],
        ID,
    )
    .description("widgets.setting.change.help"),
    SettingSpec::choice(
        "widgets.picture.fit",
        "widgets.picture.setting.fit",
        &FITS,
        FITS[0],
        ID,
    )
    .description("widgets.picture.fit.help"),
    widgets::part_spec(
        "widgets.picture.caption",
        ID,
        "widgets.picture.setting.caption",
        false,
    ),
    widgets::align_spec("widgets.picture.align", ID, Align::Center),
    widgets::frame_spec("widgets.picture.frame", ID, Frame::None),
    widgets::padding_spec("widgets.picture.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.picture.paper", ID),
    widgets::ink_spec("widgets.picture.ink", ID),
    widgets::corners_spec("widgets.picture.corners", ID),
    widgets::frame_line_spec("widgets.picture.frame-line", ID),
    widgets::shadow_spec("widgets.picture.drop-shadow", ID),
];

pub(super) fn register(r: &mut Registrar, changes: &Changes) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.picture");
        &SPEC
    })?;
    r.add_settings(&FOLDER_SETTING)?;
    r.add_settings(&SETTINGS)?;
    let changed = changes.clone();
    r.on(NEXT, move |env, _, out| {
        Turns::turn(env.storage(), &turns_file(), today(env), daily(env));
        changed.changed(out, &ID);
    })?;
    let changes = changes.clone();
    let shown = Rc::new(Showings::default());
    r.add_widget(
        WidgetSpec::new(ID, "widgets.picture", move |env| {
            // Another picture each time it shows: made as it shows.
            let showing = if daily(env) { 0 } else { shown.show(env.now()) };
            Box::new(PictureWidget {
                showing,
                changes: changes.clone(),
                ..PictureWidget::default()
            })
        })
        .category(widgets::categories::PERSONAL),
    )
}

/// Whether another picture shows each day (else each time it shows).
fn daily(env: &Env<'_>) -> bool {
    env.settings().read(&CHANGE) != CHANGES[1]
}

/// The folder of the pictures, as set.
fn folder(env: &Env<'_>) -> String {
    env.settings().read(&FOLDER).trim_matches('/').to_string()
}

/// Whether `name` is a picture shown.
fn is_picture(name: &str, is_dir: bool) -> bool {
    let lower = name.to_lowercase();
    !is_dir && !name.starts_with('.') && PICTURES.iter().any(|e| lower.ends_with(e))
}

/// The name of picture `at` of `count` in `folder`, as the folder lists
/// them (counted, not kept: a folder may hold thousands); also the
/// number of pictures there.
fn picture_at(
    storage: &dyn Storage,
    folder: &str,
    position: impl Fn(usize) -> usize,
) -> Option<(String, u64)> {
    let mut count = 0;
    let _ = storage.list(folder, &mut |entry| {
        count += usize::from(is_picture(entry.name, entry.is_dir));
    });
    if count == 0 {
        return None;
    }
    let at = position(count) % count;
    let mut index = 0;
    let mut found = None;
    let _ = storage.list(folder, &mut |entry| {
        if !is_picture(entry.name, entry.is_dir) {
            return;
        }
        if index == at {
            found = Some((entry.name.to_string(), entry.size));
        }
        index += 1;
    });
    found
}

/// A picture's name as a caption: without its ending, `_` as spaces.
fn caption(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    stem.replace('_', " ").trim().to_string()
}

/// The picture shown: decoded from its file and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Shown {
    image: ImageRef,
    name: String,
}

/// Today's picture (turned on as often as tapped and, where another
/// shows each time, as often as shown: `showing`), decoded unless it
/// was already.
fn picture(env: &Env<'_>, day: i64, showing: u32) -> Option<Shown> {
    let folder = folder(env);
    let daily = daily(env);
    let turns = Turns::load(env.storage(), &turns_file())
        .on(day, daily)
        .wrapping_add(showing);
    let (name, size) = picture_at(env.storage(), &folder, |count| {
        let first = if daily { of_the_day(day, count) } else { 0 };
        first + turns as usize
    })?;
    let path = format!("{folder}/{name}");
    let display = env.device().display;
    let color = display.format.is_color();
    // What it is decoded from and for: the same, and it is not again.
    let stamp = format!(
        "{path}\t{size}\t{}x{}\t{color}",
        display.width, display.height
    );
    let pixels = pixels_file();
    let kept = paperpaw_sdk::read_file(env.storage(), &stamp_file(), 1024)
        .is_ok_and(|kept| kept == stamp.as_bytes());
    let image = match ImageFile::open(env.storage(), &pixels).filter(|_| kept) {
        Some(file) => ImageRef {
            path: file.path,
            width: file.width,
            height: file.height,
        },
        None => {
            let fit = Fit::screen(display.width.into(), display.height.into());
            let image =
                paperpaw_sdk::image::picture_from_file(env.storage(), &path, &pixels, fit, color)?;
            let _ = env.storage().write(&stamp_file(), stamp.as_bytes());
            image
        }
    };
    Some(Shown {
        image,
        name: caption(&name),
    })
}

/// A picture of the folder, read when updated.
#[derive(Default)]
struct PictureWidget {
    /// The showing it was made for, where another shows each time.
    showing: u32,
    changes: Changes,
    shown: Option<Shown>,
    /// The day and the changes it was read at.
    read: Option<(i64, u32)>,
}

impl Widget for PictureWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        // Read each minute, but chosen anew only on another day or after
        // a tap: decoding takes a while.
        let day = today(env);
        let read = Some((day, self.changes.count()));
        if read == self.read {
            return false;
        }
        self.read = read;
        let shown = picture(env, day, self.showing);
        let changed = shown != self.shown;
        self.shown = shown;
        changed
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        // As high as wide, with the frame and the caption.
        let look = widgets::widget_look(env, ID.as_str(), Rect::new(0, 0, width, width));
        width.min(env.device().display.height / 3) + caption_height(env) + (width - look.area.width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let Some(shown) = &self.shown else {
            let message = env.i18n().format(
                "widgets.picture.none",
                &[("folder", Arg::Str(&folder(env)))],
            );
            super::draw_empty(canvas, env, &look, &message);
            return;
        };
        let area = look.area;
        // The caption where the picture keeps most of the room.
        let below = Some(caption_height(env))
            .filter(|&h| h * 3 <= area.height)
            .unwrap_or(0);
        let room = Rect::new(area.x, area.y, area.width, area.height - below);
        let place = if env.settings().read(&FIT) == FITS[1] {
            let (w, h) = ui::fit_image(&shown.image, room.width, room.height);
            let at = Rect::new(
                room.x + (room.width - w) / 2,
                room.y + (room.height - h) / 2,
                w,
                h,
            );
            ui::draw_image(canvas, env.storage(), &shown.image, at);
            at
        } else {
            draw_filled(canvas, env.storage(), &shown.image, room);
            room
        };
        if below == 0 {
            return;
        }
        let style = UiStyle::new(env);
        let Some(font) = style.font(env) else {
            return;
        };
        let mut ui = ui::Ui::new(canvas, env, &style);
        let small = widgets::line_height(env, style.small());
        // Just below the picture, wherever its whole leaves it.
        let row = Rect::new(
            area.x,
            place.y + place.height + below - small,
            area.width,
            small,
        );
        let name = ui::fit(&font, style.small(), &shown.name, f32::from(area.width));
        let x = widgets::align(env, ID.as_str()).x(area, font.measure(style.small(), &name));
        ui::Label::new(&name)
            .px(style.small())
            .color(look.faint)
            .draw_from(&mut ui, x, row);
    }

    fn targets(&self, _: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        alloc::vec![WidgetTarget::new(area, NEXT.emit(()))]
    }
}

/// The height of the caption below the picture with its gap; 0 without.
fn caption_height(env: &Env<'_>) -> u16 {
    if !widgets::shows(env, ID.as_str(), CAPTION) {
        return 0;
    }
    let style = UiStyle::new(env);
    widgets::line_height(env, style.small()) + style.gap() / 2
}

/// The part of a picture `width` × `height` that has the proportions of
/// `area`, in its middle: its left, top, width and height.
fn crop(width: u32, height: u32, area: Rect) -> (u32, u32, u32, u32) {
    let (w, h) = (u64::from(area.width.max(1)), u64::from(area.height.max(1)));
    let (iw, ih) = (u64::from(width), u64::from(height));
    let (cw, ch) = if iw * h > ih * w {
        ((ih * w / h).max(1), ih)
    } else {
        (iw, (iw * h / w).max(1))
    };
    (
        ((iw - cw) / 2) as u32,
        ((ih - ch) / 2) as u32,
        cw as u32,
        ch as u32,
    )
}

/// Draws `image` filling `area`, its edges cut where its proportions
/// differ: rows read from its file as they are drawn, as
/// [`ui::draw_image`] does, only those in the canvas's clip.
fn draw_filled(canvas: &mut Canvas<'_>, storage: &dyn Storage, image: &ImageRef, area: Rect) {
    let clip = canvas.clip();
    let Some(visible) = area.intersection(&clip) else {
        return;
    };
    let Some(file) = ImageFile::open(storage, &image.path) else {
        return;
    };
    if file.width == 0 || file.height == 0 || area.width == 0 || area.height == 0 {
        return;
    }
    let (left, top, cw, ch) = crop(file.width, file.height, area);
    let channels = if file.color { 3 } else { 1 };
    let row = file.width as usize * channels;
    // Rows read a few at a time: a card serves blocks faster.
    let rows = (8192 / row.max(1)).max(1);
    let mut data = Vec::new();
    if data.try_reserve_exact(rows * row).is_err() {
        return;
    }
    data.resize(rows * row, 0);
    let (gray, color) = (
        GrayFile {
            path: file.path.clone(),
            width: file.width,
            height: file.height,
        },
        ColorFile {
            path: file.path.clone(),
            width: file.width,
            height: file.height,
        },
    );
    let (w, h) = (u32::from(area.width), u32::from(area.height));
    let mut out = alloc::vec![0u8; w as usize * channels];
    // The first row read, and how many.
    let (mut first, mut count) = (0u32, 0u32);
    let mut made = None;
    for y in visible.y..visible.y + visible.height {
        let sy = top + (u32::from(y - area.y) * ch / h).min(ch - 1);
        if !(first..first + count).contains(&sy) {
            first = sy;
            count = if file.color {
                color.read_rows(storage, sy, &mut data)
            } else {
                gray.read_rows(storage, sy, &mut data)
            };
            if count == 0 {
                return;
            }
        }
        if made != Some(sy) {
            made = Some(sy);
            let source = &data[(sy - first) as usize * row..][..row];
            for x in 0..w {
                let from = left + x * cw / w;
                let to = (left + (x + 1) * cw / w).max(from + 1).min(left + cw);
                for c in 0..channels {
                    let sum: u32 = (from..to)
                        .map(|i| u32::from(source[i as usize * channels + c]))
                        .sum();
                    out[x as usize * channels + c] = (sum / (to - from)) as u8;
                }
            }
        }
        if file.color {
            canvas.draw_rgb_row(i32::from(area.x), i32::from(y), &out);
        } else {
            canvas.draw_gray_row(i32::from(area.x), i32::from(y), &out);
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn its_settings_are_those_read() {
        let ids: alloc::vec::Vec<&str> = super::SETTINGS.iter().map(|s| s.id.as_str()).collect();
        for read in [
            super::CHANGE.as_str(),
            super::FIT.as_str(),
            "widgets.picture.caption",
        ] {
            assert!(ids.contains(&read), "{read}");
        }
        for look in ["align", "frame"] {
            let id = alloc::format!("{}.{look}", super::ID);
            assert!(ids.contains(&id.as_str()), "{id}");
        }
        assert!(super::SETTINGS.iter().all(|s| s.group == super::ID));
    }
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    #[test]
    fn pictures_are_found_in_their_folder() {
        let storage = MemoryStorage::new()
            .with("Pictures/b.png", "x")
            .with("Pictures/a.JPG", "x")
            .with("Pictures/notes.txt", "x")
            .with("Pictures/.hidden.jpg", "x")
            .with("Pictures/sub/c.jpg", "x");
        let names: Vec<String> = (0..3)
            .map(|n| picture_at(&storage, "Pictures", |_| n).unwrap().0)
            .collect();
        // Two, round again.
        assert_eq!(names[0], names[2]);
        assert_ne!(names[0], names[1]);
        assert!(names.iter().all(|n| n == "a.JPG" || n == "b.png"));
        assert_eq!(picture_at(&storage, "Empty", |_| 0), None);
    }

    #[test]
    fn captions_are_names() {
        assert_eq!(caption("Holiday_by_the_sea.jpg"), "Holiday by the sea");
        assert_eq!(caption("Anna 2024.png"), "Anna 2024");
        assert_eq!(caption("plain"), "plain");
    }

    #[test]
    fn filling_cuts_the_edges() {
        // Wide picture, square place: its middle.
        assert_eq!(
            crop(400, 200, Rect::new(0, 0, 100, 100)),
            (100, 0, 200, 200)
        );
        // Tall picture, wide place.
        assert_eq!(
            crop(200, 400, Rect::new(0, 0, 200, 100)),
            (0, 150, 200, 100)
        );
        // The same proportions: all of it.
        assert_eq!(crop(300, 150, Rect::new(5, 5, 100, 50)), (0, 0, 300, 150));
    }
}
