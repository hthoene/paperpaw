// Made by tools/shared-table.py from shared.list: do not edit.

/// The revision of each family (`shared.list`).
pub const REVISIONS: crate::Revisions = [1, 0, 2, 1, 0, 1, 1, 0];
/// The oldest revision of each family whose plugins still run.
pub const OLDEST: crate::Revisions = [1, 0, 2, 1, 0, 1, 1, 0];
/// Eight hex digits naming the tables of this build, for logs.
pub const HASH: &str = "6e69e0a3";

/// The text family's table.
pub mod text {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::TEXT;
    /// `paperpaw_text::measure` (revision 1).
    pub const TEXT_MEASURE: usize = 0;
    /// `paperpaw_text::is_mark` (revision 1).
    pub const TEXT_IS_MARK: usize = 1;
    /// `paperpaw_text::glyph_x` (revision 1).
    pub const TEXT_GLYPH_X: usize = 2;
    /// `paperpaw_text::draw_text` (revision 1).
    pub const TEXT_DRAW_TEXT: usize = 3;
    /// `paperpaw_text::charset::stand_in` (revision 1).
    pub const CHARSET_STAND_IN: usize = 4;
    /// `paperpaw_text::Font::from_cow` (revision 1).
    pub const FONT_FROM_COW: usize = 5;
    /// `paperpaw_text::Font::size_for` (revision 1).
    pub const FONT_SIZE_FOR: usize = 6;
    /// `paperpaw_text::Font::largest_size` (revision 1).
    pub const FONT_LARGEST_SIZE: usize = 7;
    /// `paperpaw_text::Font::line_metrics` (revision 1).
    pub const FONT_LINE_METRICS: usize = 8;
    /// `paperpaw_text::Font::advance` (revision 1).
    pub const FONT_ADVANCE: usize = 9;
    /// `paperpaw_text::Font::kern` (revision 1).
    pub const FONT_KERN: usize = 10;
    /// `paperpaw_text::Font::has_char` (revision 1).
    pub const FONT_HAS_CHAR: usize = 11;
    /// `paperpaw_text::Font::shown` (revision 1).
    pub const FONT_SHOWN: usize = 12;
    /// `paperpaw_text::BitmapFont::largest_size` (revision 1).
    pub const PACK_LARGEST_SIZE: usize = 13;
    /// `paperpaw_text::BitmapFont::line_metrics` (revision 1).
    pub const PACK_LINE_METRICS: usize = 14;
    /// `paperpaw_text::BitmapFont::has_char` (revision 1).
    pub const PACK_HAS_CHAR: usize = 15;
    /// `paperpaw_text::BitmapFont::size_for` (revision 1).
    pub const PACK_SIZE_FOR: usize = 16;
    /// `paperpaw_text::BitmapFont::glyph` (revision 1).
    pub const PACK_GLYPH: usize = 17;
    /// `paperpaw_text::BitmapFont::advance` (revision 1).
    pub const PACK_ADVANCE: usize = 18;
    /// `paperpaw_text::BitmapFont::kern` (revision 1).
    pub const PACK_KERN: usize = 19;
    /// `paperpaw_text::wrap_lines_kerned_in` (revision 1).
    pub const LAYOUT_WRAP_KERNED: usize = 20;
    /// `paperpaw_text::wrap_lines_balanced_in` (revision 1).
    pub const LAYOUT_WRAP_BALANCED: usize = 21;
    /// `crate::fonts::Fonts::handle` (revision 1).
    pub const FONTS_HANDLE: usize = 22;
    /// `crate::fonts::FontHandle::largest_size` (revision 1).
    pub const HANDLE_LARGEST_SIZE: usize = 23;
    /// `crate::fonts::FontHandle::line_metrics` (revision 1).
    pub const HANDLE_LINE_METRICS: usize = 24;
    /// `crate::fonts::FontHandle::heading_line_height` (revision 1).
    pub const HANDLE_HEADING_LINE_HEIGHT: usize = 25;
    /// `crate::fonts::FontHandle::has_char` (revision 1).
    pub const HANDLE_HAS_CHAR: usize = 26;
    /// `crate::fonts::FontHandle::advance` (revision 1).
    pub const HANDLE_ADVANCE: usize = 27;
    /// `crate::fonts::FontHandle::kern` (revision 1).
    pub const HANDLE_KERN: usize = 28;
    /// `crate::fonts::FontHandle::size_for` (revision 1).
    pub const HANDLE_SIZE_FOR: usize = 29;
    /// `crate::fonts::FontHandle::measure` (revision 1).
    pub const HANDLE_MEASURE: usize = 30;
    /// `crate::fonts::FontHandle::ink` (revision 1).
    pub const HANDLE_INK: usize = 31;
    /// `crate::fonts::FontHandle::ink_across` (revision 1).
    pub const HANDLE_INK_ACROSS: usize = 32;
    /// `crate::fonts::FontHandle::wrap` (revision 1).
    pub const HANDLE_WRAP: usize = 33;
    /// `crate::fonts::FontHandle::draw` (revision 1).
    pub const HANDLE_DRAW: usize = 34;
    /// `crate::fonts::FontHandle::draw_bolder` (revision 1).
    pub const HANDLE_DRAW_BOLDER: usize = 35;
    /// `crate::fonts::FontHandle::draw_toned` (revision 1).
    pub const HANDLE_DRAW_TONED: usize = 36;
    /// `paperpaw_text::measure_in` (revision 1).
    pub const TEXT_MEASURE_IN: usize = 37;
    /// `paperpaw_text::draw_text_in` (revision 1).
    pub const TEXT_DRAW_TEXT_IN: usize = 38;
    /// `paperpaw_text::StoredPack::open_within` (revision 1).
    pub const STORED_OPEN: usize = 39;
    /// `paperpaw_text::GlyphCache::new` (revision 1).
    pub const CACHE_NEW: usize = 40;
    /// `crate::fonts::Fonts::failed_reads` (revision 1).
    pub const FONTS_FAILED_READS: usize = 41;
    /// `crate::fonts::FontHandle::measure_in` (revision 1).
    pub const HANDLE_MEASURE_IN: usize = 42;
    /// `crate::fonts::FontHandle::draw_in` (revision 1).
    pub const HANDLE_DRAW_IN: usize = 43;
    /// How many entries the table has.
    pub const COUNT: usize = 44;
}

/// The fonts family's table.
pub mod fonts {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::FONTS;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The ui family's table.
pub mod ui {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::UI;
    /// `crate::ui::style::UiStyle::new` (revision 1).
    pub const STYLE_NEW: usize = 0;
    /// `crate::ui::style::UiStyle::noted_row` (revision 1).
    pub const STYLE_NOTED_ROW: usize = 1;
    /// `crate::ui::style::draw_focus` (revision 1).
    pub const STYLE_DRAW_FOCUS: usize = 2;
    /// `crate::ui::style::draw_separator` (revision 1).
    pub const STYLE_DRAW_SEPARATOR: usize = 3;
    /// `crate::ui::shows` (revision 1).
    pub const UI_SHOWS: usize = 4;
    /// `crate::ui::text::icon` (revision 1).
    pub const TEXT_ICON: usize = 5;
    /// `crate::ui::text::draw_icon` (revision 1).
    pub const TEXT_DRAW_ICON: usize = 6;
    /// `crate::ui::text::icon_width` (revision 1).
    pub const TEXT_ICON_WIDTH: usize = 7;
    /// `crate::ui::text::draw_icon_at` (revision 1).
    pub const TEXT_DRAW_ICON_AT: usize = 8;
    /// `crate::ui::text::heading_height` (revision 1).
    pub const TEXT_HEADING_HEIGHT: usize = 9;
    /// `crate::ui::text::draw_heading` (revision 1).
    pub const TEXT_DRAW_HEADING: usize = 10;
    /// `crate::ui::text::icon_room` (revision 1).
    pub const TEXT_ICON_ROOM: usize = 11;
    /// `crate::ui::text::draw_row_icon` (revision 1).
    pub const TEXT_DRAW_ROW_ICON: usize = 12;
    /// `crate::ui::text::tr_input` (revision 1).
    pub const TEXT_TR_INPUT: usize = 13;
    /// `crate::ui::text::draw_label` (revision 1).
    pub const TEXT_DRAW_LABEL: usize = 14;
    /// `crate::ui::text::draw_label_in` (revision 1).
    pub const TEXT_DRAW_LABEL_IN: usize = 15;
    /// `crate::ui::text::draw_text` (revision 1).
    pub const TEXT_DRAW_TEXT: usize = 16;
    /// `crate::ui::text::put_run` (revision 1).
    pub const TEXT_PUT_RUN: usize = 17;
    /// `crate::ui::text::cap_baseline` (revision 1).
    pub const TEXT_CAP_BASELINE: usize = 18;
    /// `crate::ui::text::draw_label_on` (revision 1).
    pub const TEXT_DRAW_LABEL_ON: usize = 19;
    /// `crate::ui::text::fit_pair` (revision 1).
    pub const TEXT_FIT_PAIR: usize = 20;
    /// `crate::ui::text::message_height` (revision 1).
    pub const TEXT_MESSAGE_HEIGHT: usize = 21;
    /// `crate::ui::text::draw_message` (revision 1).
    pub const TEXT_DRAW_MESSAGE: usize = 22;
    /// `crate::ui::text::note_line_height` (revision 1).
    pub const TEXT_NOTE_LINE_HEIGHT: usize = 23;
    /// `crate::ui::text::draw_note_line` (revision 1).
    pub const TEXT_DRAW_NOTE_LINE: usize = 24;
    /// `crate::ui::text::fit` (revision 1).
    pub const TEXT_FIT: usize = 25;
    /// `crate::ui::text::empty_height` (revision 1).
    pub const TEXT_EMPTY_HEIGHT: usize = 26;
    /// `crate::ui::text::draw_empty` (revision 1).
    pub const TEXT_DRAW_EMPTY: usize = 27;
    /// `crate::ui::button::draw_button_said` (revision 1).
    pub const BUTTON_DRAW_SAID: usize = 28;
    /// `crate::ui::button::button_frame` (revision 1).
    pub const BUTTON_FRAME: usize = 29;
    /// `crate::ui::button::button_label` (revision 1).
    pub const BUTTON_LABEL: usize = 30;
    /// `crate::ui::button::button_height_for` (revision 1).
    pub const BUTTON_HEIGHT_FOR: usize = 31;
    /// `crate::ui::header::Header::new` (revision 1).
    pub const HEADER_NEW: usize = 32;
    /// `crate::ui::header::Header::draw` (revision 1).
    pub const HEADER_DRAW: usize = 33;
    /// `crate::ui::header::Header::tap` (revision 1).
    pub const HEADER_TAP: usize = 34;
    /// `crate::ui::header::draw_arrow` (revision 1).
    pub const HEADER_DRAW_ARROW: usize = 35;
    /// `crate::ui::row::Row::height` (revision 1).
    pub const ROW_HEIGHT: usize = 36;
    /// `crate::ui::row::Row::draw` (revision 1).
    pub const ROW_DRAW: usize = 37;
    /// `crate::ui::row::Row::hit` (revision 1).
    pub const ROW_HIT: usize = 38;
    /// `crate::ui::label::Label::height` (revision 1).
    pub const LABEL_HEIGHT: usize = 39;
    /// `crate::ui::label::Label::draw_from` (revision 1).
    pub const LABEL_DRAW_FROM: usize = 40;
    /// `crate::ui::setting_rows::title_lines` (revision 1).
    pub const SETTING_TITLE_LINES: usize = 41;
    /// `crate::ui::setting_rows::noted_row_height` (revision 1).
    pub const SETTING_NOTED_ROW_HEIGHT: usize = 42;
    /// `crate::ui::setting_rows::draw_note_below` (revision 1).
    pub const SETTING_DRAW_NOTE_BELOW: usize = 43;
    /// `crate::ui::setting_rows::draw_value_row` (revision 1).
    pub const SETTING_DRAW_VALUE_ROW: usize = 44;
    /// `crate::ui::list::ListView::set_focus` (revision 1).
    pub const LIST_SET_FOCUS: usize = 45;
    /// `crate::ui::list::ListView::focus_change` (revision 1).
    pub const LIST_FOCUS_CHANGE: usize = 46;
    /// `crate::ui::list::ListView::paging` (revision 1).
    pub const LIST_PAGING: usize = 47;
    /// `crate::ui::list::ListView::move_focus` (revision 1).
    pub const LIST_MOVE_FOCUS: usize = 48;
    /// `crate::ui::list::ListView::row_rect` (revision 1).
    pub const LIST_ROW_RECT: usize = 49;
    /// `crate::ui::list::ListView::hit` (revision 1).
    pub const LIST_HIT: usize = 50;
    /// `crate::ui::list::list_step` (revision 1).
    pub const LIST_STEP: usize = 51;
    /// How many entries the table has.
    pub const COUNT: usize = 52;
}

/// The graphics family's table.
pub mod graphics {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::GRAPHICS;
    /// `paperpaw_graphics::Canvas::clear` (revision 1).
    pub const CANVAS_CLEAR: usize = 0;
    /// `paperpaw_graphics::Canvas::fill` (revision 1).
    pub const CANVAS_FILL: usize = 1;
    /// `paperpaw_graphics::Canvas::fill_rect` (revision 1).
    pub const CANVAS_FILL_RECT: usize = 2;
    /// `paperpaw_graphics::Canvas::fill_tint` (revision 1).
    pub const CANVAS_FILL_TINT: usize = 3;
    /// `paperpaw_graphics::Canvas::fill_shaded` (revision 1).
    pub const CANVAS_FILL_SHADED: usize = 4;
    /// `paperpaw_graphics::Canvas::fill_round_shaded` (revision 1).
    pub const CANVAS_FILL_ROUND_SHADED: usize = 5;
    /// `paperpaw_graphics::Canvas::stroke` (revision 1).
    pub const CANVAS_STROKE: usize = 6;
    /// `paperpaw_graphics::Canvas::fill_round` (revision 1).
    pub const CANVAS_FILL_ROUND: usize = 7;
    /// `paperpaw_graphics::Canvas::stroke_round` (revision 1).
    pub const CANVAS_STROKE_ROUND: usize = 8;
    /// `paperpaw_graphics::Canvas::stroke_rect` (revision 1).
    pub const CANVAS_STROKE_RECT: usize = 9;
    /// `paperpaw_graphics::Canvas::fill_round_rect` (revision 1).
    pub const CANVAS_FILL_ROUND_RECT: usize = 10;
    /// `paperpaw_graphics::Canvas::stroke_round_rect` (revision 1).
    pub const CANVAS_STROKE_ROUND_RECT: usize = 11;
    /// `paperpaw_graphics::Canvas::draw_line` (revision 1).
    pub const CANVAS_DRAW_LINE: usize = 12;
    /// `paperpaw_graphics::Canvas::fill_circle` (revision 1).
    pub const CANVAS_FILL_CIRCLE: usize = 13;
    /// `paperpaw_graphics::Canvas::stroke_circle` (revision 1).
    pub const CANVAS_STROKE_CIRCLE: usize = 14;
    /// `paperpaw_graphics::Canvas::fill_ring` (revision 1).
    pub const CANVAS_FILL_RING: usize = 15;
    /// `paperpaw_graphics::Canvas::draw_gray_row` (revision 1).
    pub const CANVAS_DRAW_GRAY_ROW: usize = 16;
    /// `paperpaw_graphics::Canvas::draw_gray_alpha_row` (revision 1).
    pub const CANVAS_DRAW_GRAY_ALPHA_ROW: usize = 17;
    /// `paperpaw_graphics::Canvas::draw_rgb_row` (revision 1).
    pub const CANVAS_DRAW_RGB_ROW: usize = 18;
    /// `paperpaw_graphics::Canvas::clipped` (revision 1).
    pub const CANVAS_CLIPPED: usize = 19;
    /// `paperpaw_graphics::Canvas::describe` (revision 1).
    pub const CANVAS_DESCRIBE: usize = 20;
    /// `paperpaw_graphics::Canvas::note_picture` (revision 1).
    pub const CANVAS_NOTE_PICTURE: usize = 21;
    /// `paperpaw_graphics::Canvas::keep_diffusion` (revision 1).
    pub const CANVAS_KEEP_DIFFUSION: usize = 22;
    /// `paperpaw_graphics::Canvas::take_diffusion` (revision 1).
    pub const CANVAS_TAKE_DIFFUSION: usize = 23;
    /// `paperpaw_graphics::round::corner_inset` (revision 1).
    pub const ROUND_CORNER_INSET: usize = 24;
    /// `paperpaw_graphics::round::sin_cos` (revision 1).
    pub const ROUND_SIN_COS: usize = 25;
    /// `paperpaw_graphics::round::point_at` (revision 1).
    pub const ROUND_POINT_AT: usize = 26;
    /// How many entries the table has.
    pub const COUNT: usize = 27;
}

/// The contribution family's table.
pub mod contribution {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::CONTRIBUTION;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The data family's table.
pub mod data {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::DATA;
    /// `paperpaw_i18n::Translator::get` (revision 1).
    pub const I18N_GET: usize = 0;
    /// `paperpaw_i18n::Translator::format` (revision 1).
    pub const I18N_FORMAT: usize = 1;
    /// `paperpaw_i18n::Translator::plural` (revision 1).
    pub const I18N_PLURAL: usize = 2;
    /// `paperpaw_i18n::Catalog::get` (revision 1).
    pub const I18N_CATALOG_GET: usize = 3;
    /// `crate::settings::Settings::get` (revision 1).
    pub const SETTINGS_GET: usize = 4;
    /// `crate::settings::Settings::scoped_value` (revision 1).
    pub const SETTINGS_SCOPED_VALUE: usize = 5;
    /// `crate::settings::Settings::text` (revision 1).
    pub const SETTINGS_TEXT: usize = 6;
    /// `paperpaw_json::object` (revision 1).
    pub const JSON_OBJECT: usize = 7;
    /// `paperpaw_json::text` (revision 1).
    pub const JSON_TEXT: usize = 8;
    /// `paperpaw_json::number` (revision 1).
    pub const JSON_NUMBER: usize = 9;
    /// `paperpaw_json::string` (revision 1).
    pub const JSON_STRING: usize = 10;
    /// `paperpaw_json::raw_member` (revision 1).
    pub const JSON_RAW_MEMBER: usize = 11;
    /// `paperpaw_json::unquote` (revision 1).
    pub const JSON_UNQUOTE: usize = 12;
    /// `paperpaw_json::string_array` (revision 1).
    pub const JSON_STRING_ARRAY: usize = 13;
    /// `paperpaw_json::stream::Parser::feed` (revision 1).
    pub const JSON_STREAM_FEED: usize = 14;
    /// `paperpaw_json::stream::Parser::finish` (revision 1).
    pub const JSON_STREAM_FINISH: usize = 15;
    /// `paperpaw_json::stream::read_file` (revision 1).
    pub const JSON_READ_FILE: usize = 16;
    /// `paperpaw_config::Parser::next` (revision 1).
    pub const CONFIG_NEXT: usize = 17;
    /// `paperpaw_config::Value::as_str` (revision 1).
    pub const CONFIG_AS_STR: usize = 18;
    /// `paperpaw_config::Value::as_i64` (revision 1).
    pub const CONFIG_AS_I64: usize = 19;
    /// `paperpaw_config::Value::as_u32` (revision 1).
    pub const CONFIG_AS_U32: usize = 20;
    /// `paperpaw_config::set` (revision 1).
    pub const CONFIG_SET: usize = 21;
    /// `paperpaw_config::remove` (revision 1).
    pub const CONFIG_REMOVE: usize = 22;
    /// `paperpaw_config::quote` (revision 1).
    pub const CONFIG_QUOTE: usize = 23;
    /// `paperpaw_xml::Tokenizer::parse` (revision 1).
    pub const XML_PARSE: usize = 24;
    /// `paperpaw_xml::Tokenizer::feed` (revision 1).
    pub const XML_FEED: usize = 25;
    /// `paperpaw_xml::Tokenizer::try_feed` (revision 1).
    pub const XML_TRY_FEED: usize = 26;
    /// `paperpaw_xml::Tokenizer::finish` (revision 1).
    pub const XML_FINISH: usize = 27;
    /// `paperpaw_xml::Attrs::get` (revision 1).
    pub const XML_ATTRS_GET: usize = 28;
    /// `paperpaw_xml::decode` (revision 1).
    pub const XML_DECODE: usize = 29;
    /// How many entries the table has.
    pub const COUNT: usize = 30;
}

/// The widgets family's table.
pub mod widgets {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::WIDGETS;
    /// `crate::widgets::look::widget_look` (revision 1).
    pub const WIDGET_LOOK: usize = 0;
    /// `crate::widgets::look::draw_frame` (revision 1).
    pub const WIDGET_DRAW_FRAME: usize = 1;
    /// `crate::widgets::look::frame_height` (revision 1).
    pub const WIDGET_FRAME_HEIGHT: usize = 2;
    /// `crate::widgets::look::draw_caption` (revision 1).
    pub const WIDGET_DRAW_CAPTION: usize = 3;
    /// How many entries the table has.
    pub const COUNT: usize = 4;
}
