//! Book information: cover, title and author, the series and tags, the
//! language, how far it is read, the fields plugins keep with books
//! ([`paperpaw_sdk::library::BookField`]), the file, and what the book is
//! about.

use alloc::borrow::Cow;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{actions, params};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Cx, Env, Gesture, GestureKind, Handled, Id, Params, Rect, Screen,
    files,
};

use paperpaw_sdk::library;

/// What is shown of a book.
#[derive(Debug, Default)]
struct Details {
    title: String,
    author: String,
    cover: Option<ImageRef>,
    /// Label key and value, in order.
    rows: Vec<(Cow<'static, str>, String)>,
}

pub(crate) struct InfoScreen {
    style: UiStyle,
    header: ui::Header,
    details: Details,
    layout: Layout,
}

impl InfoScreen {
    pub(crate) fn new(env: &Env<'_>, params: &Params) -> Self {
        let style = UiStyle::new(env);
        let path = params.get(params::PATH).unwrap_or_default();
        let header = ui::Header::new(env, &style, true);
        let (details, description) = details(env, path);
        let layout = Layout::new(env, &style, header.area().height, &details, &description);
        Self {
            header,
            details,
            layout,
            style,
        }
    }
}

/// Where everything goes, worked out when the screen opens: drawn in
/// every band, nothing is measured, wrapped or read then.
#[derive(Default)]
struct Layout {
    /// The cover, where it goes and made small for it (`None` where no
    /// memory was left: then an outline stands in, as reading the picture
    /// in every band would take long).
    cover: Option<(Rect, Option<Thumbnail>)>,
    /// Where the title and the author begin from the left.
    text_x: u16,
    /// The title's lines, at most two, each with its top.
    title: Vec<(u16, String)>,
    /// The author's top.
    author: u16,
    /// The top of the first of the labelled rows.
    rows: u16,
    /// The labels' column, the values right of it.
    label_width: f32,
    /// What the book is about, as many lines as fit (the last ending in
    /// "…" where there is more), and the top of the first.
    description: Vec<String>,
    description_top: u16,
}

impl Layout {
    fn new(env: &Env<'_>, style: &UiStyle, header: u16, d: &Details, description: &str) -> Self {
        let Some(font) = style.font(env) else {
            return Self::default();
        };
        let display = env.device().display;
        let margin = style.margin();
        let gap = style.gap();
        let width = display.width.saturating_sub(2 * margin);
        let line = |size: f32| font.line_metrics(size).line_height as u16;
        let mut y = header + gap * 2;

        // The cover beside title and author.
        let cover_height = style.row() * 2;
        let cover = d.cover.as_ref().map(|image| {
            let (w, h) = ui::fit_image(image, cover_height * 3 / 4, cover_height);
            let at = Rect::new(margin, y, w, h);
            let format = env.device().display.format;
            (at, Thumbnail::new(env.storage(), image, w, h, format))
        });
        let text_x = cover
            .as_ref()
            .map_or(margin, |(at, _)| margin + at.width + gap * 2);
        let text_width = display.width.saturating_sub(text_x + margin);
        let mut ty = y;
        let mut title = Vec::new();
        let ranges = font.wrap(style.title(), &d.title, f32::from(text_width));
        let count = ranges.len().min(2);
        for (i, range) in ranges.into_iter().take(2).enumerate() {
            title.push((ty, d.title[range].to_string()));
            // Its lines a heading's apart, the author a line below.
            ty += if i + 1 < count {
                paperpaw_sdk::widgets::heading_line_height(env, style.title())
            } else {
                line(style.title())
            };
        }
        let author = ty;
        if !d.author.is_empty() {
            ty += line(style.text());
        }
        y = ty.max(if cover.is_some() { y + cover_height } else { y }) + gap * 2;

        // Label and value in two columns, the values aligned.
        let label_width = d
            .rows
            .iter()
            .map(|(key, _)| font.measure(style.small(), env.i18n().tr(key.as_ref())))
            .fold(0.0, f32::max)
            + f32::from(gap) * 2.0;
        let rows = y;
        y += (line(style.text()) + gap / 2) * d.rows.len() as u16;

        // What the book is about, as far as there is room.
        let mut shown = Vec::new();
        let description_top = y + gap * 2;
        if !description.is_empty() {
            let bottom = display.height.saturating_sub(margin);
            let lines = font.wrap(style.text(), description, f32::from(width));
            let fits =
                usize::from(bottom.saturating_sub(description_top) / line(style.text()).max(1));
            for (i, range) in lines.iter().take(fits).enumerate() {
                let mut text = description[range.clone()].to_string();
                if i + 1 == fits && lines.len() > fits {
                    text.push('…');
                }
                shown.push(text);
            }
        }
        Self {
            cover,
            text_x,
            title,
            author,
            rows,
            label_width,
            description: shown,
            description_top,
        }
    }
}

/// Reads what is known of the book at `path`, and what it is about.
fn details(env: &Env<'_>, path: &str) -> (Details, String) {
    let storage = env.storage();
    let name = files::name(path);
    let cached = library::service(env).book(env, path);
    let mut details = Details {
        title: library::readable_name(name.rsplit_once('.').map_or(name, |(n, _)| n)).into_owned(),
        ..Details::default()
    };
    let mut rows = Vec::new();
    let mut description = String::new();
    if let Ok(document) = env.formats().open(storage, path) {
        let metadata = document.metadata().clone();
        if let Some(title) = metadata.title {
            details.title = title;
        }
        details.author = metadata.authors.join(", ");
        details.cover = document.cover(storage).ok().flatten();
        if let Some(series) = metadata.series {
            let value = match metadata.series_index {
                Some(number) => format!(
                    "{series} · {}",
                    env.i18n()
                        .format("files.series-number", &[("number", Arg::Str(&number))])
                ),
                None => series,
            };
            rows.push((Cow::Borrowed("files.info.series"), value));
        }
        if !metadata.subjects.is_empty() {
            rows.push((
                Cow::Borrowed("files.info.tags"),
                metadata.subjects.join(", "),
            ));
        }
        if let Some(language) = metadata.language {
            rows.push((Cow::Borrowed("files.info.language"), language));
        }
        description = metadata.description.unwrap_or_default();
    }
    if let Some(progress) = cached.as_ref().and_then(|b| b.progress) {
        rows.push((
            Cow::Borrowed("files.info.progress"),
            format!("{progress} %"),
        ));
    }
    // The fields plugins keep with books, where this one has a value.
    if let Some(book) = &cached {
        for field in env.book_fields() {
            if let Some(value) = book.field(field.id.as_str()) {
                rows.push((field.title.clone(), value.to_string()));
            }
        }
    }
    let size = storage.size(path).unwrap_or_default();
    let file = file_row(name, size, env.i18n().tr("files.decimal"));
    rows.push((Cow::Borrowed("files.info.file"), file));
    rows.push((
        Cow::Borrowed("files.info.folder"),
        files::parent(path).to_string(),
    ));
    details.rows = rows;
    (details, description)
}

/// The file's row: its format and size, `EPUB · 2,4 MB` (with the
/// language's decimal mark).
fn file_row(name: &str, size: u64, decimal: &str) -> String {
    let (_, format) = crate::stem_and_format(name);
    format!("{format} · {}", files::size(size, decimal))
}

impl Screen for InfoScreen {
    /// Only *Back* shows: nothing else does more than close it.
    fn hint(&self, action: &Id, _: &Env<'_>) -> paperpaw_sdk::Hint {
        paperpaw_sdk::Hint::only_back(action)
    }

    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        let style = self.style;
        let mut ui = ui::Ui::new(canvas, env, &style);
        self.header
            .draw(&mut ui, env.i18n().tr("files.info"), None, None);
        let Some(font) = style.font(env) else {
            return;
        };
        let display = env.device().display;
        let margin = style.margin();
        let gap = style.gap();
        let width = display.width.saturating_sub(2 * margin);
        let x = f32::from(margin);
        let line = |size: f32| font.line_metrics(size).line_height as u16;
        let (d, layout) = (&self.details, &self.layout);

        // The cover beside title and author.
        match (&layout.cover, &d.cover) {
            (Some((at, Some(small))), _) => {
                small.draw_round(ui.canvas(), at.x, at.y, crate::picture_radius(&style, *at));
            }
            (Some((at, None)), _) => {
                ui::stroke_round(
                    ui.canvas(),
                    &style,
                    *at,
                    ui::button_border(&style),
                    style.faint(ui::Ink::DETAIL),
                );
            }
            _ => {}
        }
        let text_x = layout.text_x;
        let text_width = display.width.saturating_sub(text_x + margin);
        for (top, text) in &layout.title {
            let row = Rect::new(text_x, *top, text_width, line(style.title()));
            let x = f32::from(text_x);
            ui::Label::new(text)
                .px(style.title())
                .color(Color::BLACK)
                .draw_from(&mut ui, x, row);
        }
        if !d.author.is_empty() {
            let row = Rect::new(text_x, layout.author, text_width, line(style.text()));
            ui::Label::new(&d.author)
                .px(style.text())
                .color(style.faint(ui::Ink::DETAIL))
                .draw_from(&mut ui, f32::from(text_x), row);
        }

        // Label and value in two columns, the values aligned; label and
        // value share a baseline.
        let ascent = font.line_metrics(style.text()).ascent;
        let mut y = layout.rows;
        for (key, value) in &d.rows {
            let baseline = i32::from(y) + ascent as i32;
            let label = env.i18n().tr(key.as_ref());
            let row = Rect::new(margin, y, width, line(style.text()));
            let canvas = ui.canvas();
            canvas.describe(Role::Text, label, row);
            canvas.describe(Role::Text, value, row);
            font.draw(
                canvas,
                style.small(),
                x,
                baseline,
                label,
                style.faint(ui::Ink::DETAIL),
            );
            let room = f32::from(width) - layout.label_width;
            let value = ui::fit(&font, style.text(), value, room);
            font.draw(
                canvas,
                style.text(),
                x + layout.label_width,
                baseline,
                &value,
                Color::BLACK,
            );
            y += line(style.text()) + gap / 2;
        }

        // What the book is about, as far as there is room.
        let mut y = layout.description_top;
        for text in &layout.description {
            let row = Rect::new(margin, y, width, line(style.text()));
            ui::Label::new(text)
                .px(style.text())
                .color(Color::BLACK)
                .draw_from(&mut ui, x, row);
            y += line(style.text());
        }
    }

    fn on_action(&mut self, action: &Id, cx: &mut Cx<'_, '_>) -> Handled {
        if [actions::BACK, actions::CONFIRM, actions::MENU].contains(action) {
            cx.command(Command::Back);
            return Handled::Yes;
        }
        Handled::No
    }

    fn on_gesture(&mut self, gesture: Gesture, cx: &mut Cx<'_, '_>) -> Handled {
        if gesture.kind != GestureKind::Tap {
            return Handled::No;
        }
        if let Some(action) = self.header.tap(cx.env(), &self.style, gesture.start, None) {
            cx.command(Command::run(action));
            return Handled::Yes;
        }
        Handled::No
    }
}

#[cfg(test)]
mod tests {
    use super::file_row;

    #[test]
    fn sizes_read_easily_in_each_language() {
        assert_eq!(file_row("a.epub", 340 * 1024, "."), "EPUB · 340 KB");
        assert_eq!(file_row("a.epub", 2_516_582, ","), "EPUB · 2,4 MB");
        assert_eq!(file_row("a.fb2.zip", 2_516_582, "."), "FB2 · 2.4 MB");
    }
}
