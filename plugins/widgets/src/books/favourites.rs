//! "Favourites": the favourite books as covers, the newest first, in a
//! row or in rows.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::library;
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetLook, WidgetTarget,
};
use paperpaw_sdk::{Canvas, Env, Id, Rect, RegisterError, Registrar};

use super::shelf::{self, Place, Shelved};
use super::{Book, Covers, add_settings, chosen, count, current, draw_message, frame_height};
use paperpaw_sdk::SettingSpec;

pub(crate) const ID: Id = Id::new_static(WIDGET);
/// Its id, as text.
const WIDGET: &str = "widgets.favourites";
/// In a row, or in rows filling the place.
pub(crate) const LAYOUTS: [&str; 2] = ["widgets.books.layout.row", "widgets.books.layout.grid"];

/// Its settings, kept in the program.
static SETTINGS: [SettingSpec; 12] = [
    widgets::align_spec("widgets.favourites.align", ID, Align::Center),
    widgets::frame_spec("widgets.favourites.frame", ID, Frame::None),
    widgets::padding_spec("widgets.favourites.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.favourites.paper", ID),
    widgets::ink_spec("widgets.favourites.ink", ID),
    widgets::size_spec("widgets.favourites.size", ID, TextSize::Medium, false),
    SettingSpec::choice(
        "widgets.favourites.layout",
        "widgets.books.setting.layout",
        &LAYOUTS,
        LAYOUTS[0],
        ID,
    )
    .description("widgets.favourites.layout.help"),
    SettingSpec::number(
        "widgets.favourites.count",
        "widgets.books.setting.count",
        1,
        24,
        8,
        ID,
    )
    .description("widgets.setting.count.help"),
    widgets::part_spec(
        "widgets.favourites.caption",
        ID,
        "widgets.books.part.caption",
        true,
    ),
    widgets::part_spec(
        "widgets.favourites.titles",
        ID,
        "widgets.books.part.titles",
        false,
    ),
    widgets::part_spec(
        "widgets.favourites.read",
        ID,
        "widgets.books.part.read",
        false,
    ),
    widgets::part_spec(
        "widgets.favourites.current",
        ID,
        "widgets.books.part.current",
        false,
    ),
];

pub(crate) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    add_settings(
        r,
        {
            static SPEC: paperpaw_sdk::SettingGroupSpec =
                paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.favourites");
            &SPEC
        },
        &SETTINGS,
    )?;
    static WIDGET: [FixedWidget; 1] =
        [FixedWidget::new(
            ID,
            "widgets.favourites",
            |_| Box::new(Favourites::default()),
        )
        .category(widgets::categories::BOOKS)];
    r.add_widgets(&WIDGET)
}

#[derive(Debug, Default)]
struct Favourites {
    books: Vec<Shelved>,
    /// Where it is drawn, once placed.
    area: Option<Rect>,
    covers: Covers,
}

impl Favourites {
    fn grid(env: &Env<'_>) -> bool {
        chosen(env, WIDGET, "layout") == LAYOUTS[1]
    }

    fn places(&self, env: &Env<'_>, area: Rect) -> Vec<Place> {
        let label = if widgets::shows(env, WIDGET, "titles") {
            shelf::label_height(env)
        } else {
            0
        };
        let align = widgets::align(env, WIDGET);
        let mut places = shelf::places(env, area, self.books.len(), Self::grid(env), label, align);
        shelf::pack(env, &mut places, &self.books, area, align);
        places
    }

    /// The covers shown, made small for where they go.
    fn make_covers(&mut self, env: &Env<'_>) {
        self.covers = match self.area {
            Some(area) => {
                let look = widgets::widget_look(env, WIDGET, area);
                let places = self.places(env, Self::content(env, &look));
                shelf::covers(env, &places, &self.books)
            }
            None => Covers::default(),
        };
    }

    fn content(env: &Env<'_>, look: &WidgetLook) -> Rect {
        if widgets::shows(env, WIDGET, "caption")
            && look.area.height >= 3 * widgets::caption_height(env)
        {
            super::below(look.area, widgets::caption_height(env))
        } else {
            look.area
        }
    }
}

/// The first `count` favourites still there, as shown.
fn favourites(env: &Env<'_>, count: usize) -> Vec<Shelved> {
    let id = WIDGET;
    let titles = widgets::shows(env, id, "titles");
    let read = widgets::shows(env, id, "read");
    let reading = widgets::shows(env, id, "current")
        .then(|| current(env.storage()).map(|b| b.path))
        .flatten();
    library::favourites(env.storage())
        .iter()
        .filter_map(|path| Book::of(env.storage(), path))
        .take(count)
        .map(|book| Shelved {
            label: if titles {
                book.title.clone()
            } else {
                String::new()
            },
            read: read && book.finished(),
            current: reading.as_deref() == Some(book.path.as_str()),
            book,
        })
        .collect()
}

impl Widget for Favourites {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let books = favourites(env, count(env, WIDGET, "count").min(24));
        let changed = books != self.books;
        self.books = books;
        if changed {
            self.make_covers(env);
        }
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.make_covers(env);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        let style = UiStyle::new(env);
        let columns = self.books.len().clamp(3, 6) as u16;
        let cover = (width / columns).saturating_sub(style.gap()) * 3 / 2;
        let rows = if Self::grid(env) {
            (self.books.len() as u16).div_ceil(columns).max(1)
        } else {
            1
        };
        (cover + shelf::label_height(env)) * rows
            + widgets::caption_height(env)
            + frame_height(env, WIDGET)
    }

    fn min_height(&self, env: &Env<'_>, width: u16) -> u16 {
        self.height(env, width) / 2
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, WIDGET, area);
        if self.books.is_empty() {
            draw_message(canvas, env, &look, "widgets.favourites.none");
            return;
        }
        let title = env.i18n().tr("widgets.favourites");
        let rest = Self::content(env, &look);
        let places = self.places(env, rest);
        shelf::draw_caption(canvas, env, WIDGET, &look, rest, &places, title);
        shelf::draw(canvas, env, &look, &places, &self.books, &self.covers);
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let look = widgets::widget_look(env, WIDGET, area);
        let rest = Self::content(env, &look);
        self.places(env, rest)
            .into_iter()
            .zip(&self.books)
            .map(|(place, shelved)| {
                WidgetTarget::new(place.cover.union(&place.label), shelved.book.open())
            })
            .collect()
    }
}
