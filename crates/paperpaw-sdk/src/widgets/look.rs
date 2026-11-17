//! A look widgets share, so that widgets of different plugins sit well
//! together on a page and the reader shapes them all alike: a frame
//! around a widget ([`Frame`]: none, a line, a light card, a dark card
//! with light text), how far its content keeps from the frame
//! ([`Padding`]), how large its text is ([`TextSize`]), a small caption
//! above it ([`draw_caption`]), and switches for what it shows
//! ([`part_setting`]). Each is a setting in the widget's settings group
//! ([`settings_group`](super::settings_group)), named after the widget:
//! `<widget>.frame`, `<widget>.padding`, `<widget>.size`,
//! `<widget>.<part>`.
//!
//! A widget draws its frame first and then its content into the area the
//! frame leaves, in the frame's colours:
//!
//! ```ignore
//! fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
//!     let look = widgets::draw_frame(canvas, env, ID, area);
//!     let size = widgets::size(env, ID).text(&UiStyle::new(env));
//!     // ... text in look.ink, notes in look.faint, inside look.area
//! }
//! ```
//!
//! **The shared look is a helper, never a must.** A widget may draw all
//! of itself: its own frame, plate, shape and colours, without
//! [`draw_frame`] or [`widget_look`], and then no setting for widgets
//! (the reader's own or those of all widgets: frame, paper, ink,
//! corners, frame line, shadow, spacing) changes what it draws; a grid
//! lays only the screen's [`Plate`](super::Plate) under it. A widget
//! using the helpers may still take what it likes: make its own
//! [`WidgetLook`] ([`WidgetLook::new`]), change the fields of the one it
//! is given, or leave out the settings it does not offer (each reads as
//! automatic). A theme or look plugin paints every frame itself with a
//! frame painter ([`Painters::frame`](crate::ui::look::Painters)); it is
//! given the widget's corners and line in the style, and a paper the
//! reader chose for a widget is painted instead of it. Since interface 6,
//! revision 1 of the `widgets` family, [`widget_look`], [`draw_frame`], [`frame_height`] and
//! [`draw_caption`] run the firmware's code (shared functions, ADR 0026),
//! so the look stays alike across plugins and a later change reaches
//! plugins built from then on without a rebuild.
//!
//! Text in a widget fits its place rather than running out of it: what
//! must stay on one line (a time, a date, a number, a title in a small
//! cell) is made smaller down to a least size and only then shortened
//! with "…" ([`fit_line`], [`draw_line`]); text that may take several
//! lines breaks between words, never inside one, and is made smaller
//! before it is cut ([`Wrapped::fitting`]). Sizes are measured as they
//! are drawn, so fonts with a few sizes only (the packs of small
//! devices) fit as well as fonts drawn at any size ([`largest_size`]).






















































