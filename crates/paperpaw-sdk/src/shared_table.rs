// Made by tools/shared-table.py from shared.list: do not edit.
#![cfg_attr(rustfmt, rustfmt::skip)]

pub(super) static TEXT: super::Addresses<44> = super::Addresses([
    paperpaw_text::measure as *const (), // TEXT_MEASURE
    paperpaw_text::is_mark as *const (), // TEXT_IS_MARK
    paperpaw_text::glyph_x as *const (), // TEXT_GLYPH_X
    paperpaw_text::draw_text as *const (), // TEXT_DRAW_TEXT
    paperpaw_text::charset::stand_in as *const (), // CHARSET_STAND_IN
    paperpaw_text::Font::from_cow as *const (), // FONT_FROM_COW
    paperpaw_text::Font::size_for as *const (), // FONT_SIZE_FOR
    paperpaw_text::Font::largest_size as *const (), // FONT_LARGEST_SIZE
    paperpaw_text::Font::line_metrics as *const (), // FONT_LINE_METRICS
    paperpaw_text::Font::advance as *const (), // FONT_ADVANCE
    paperpaw_text::Font::kern as *const (), // FONT_KERN
    paperpaw_text::Font::has_char as *const (), // FONT_HAS_CHAR
    paperpaw_text::Font::shown as *const (), // FONT_SHOWN
    paperpaw_text::BitmapFont::largest_size as *const (), // PACK_LARGEST_SIZE
    paperpaw_text::BitmapFont::line_metrics as *const (), // PACK_LINE_METRICS
    paperpaw_text::BitmapFont::has_char as *const (), // PACK_HAS_CHAR
    paperpaw_text::BitmapFont::size_for as *const (), // PACK_SIZE_FOR
    paperpaw_text::BitmapFont::glyph as *const (), // PACK_GLYPH
    paperpaw_text::BitmapFont::advance as *const (), // PACK_ADVANCE
    paperpaw_text::BitmapFont::kern as *const (), // PACK_KERN
    paperpaw_text::wrap_lines_kerned_in as *const (), // LAYOUT_WRAP_KERNED
    paperpaw_text::wrap_lines_balanced_in as *const (), // LAYOUT_WRAP_BALANCED
    crate::fonts::Fonts::handle as *const (), // FONTS_HANDLE
    crate::fonts::FontHandle::largest_size as *const (), // HANDLE_LARGEST_SIZE
    crate::fonts::FontHandle::line_metrics as *const (), // HANDLE_LINE_METRICS
    crate::fonts::FontHandle::heading_line_height as *const (), // HANDLE_HEADING_LINE_HEIGHT
    crate::fonts::FontHandle::has_char as *const (), // HANDLE_HAS_CHAR
    crate::fonts::FontHandle::advance as *const (), // HANDLE_ADVANCE
    crate::fonts::FontHandle::kern as *const (), // HANDLE_KERN
    crate::fonts::FontHandle::size_for as *const (), // HANDLE_SIZE_FOR
    crate::fonts::FontHandle::measure as *const (), // HANDLE_MEASURE
    crate::fonts::FontHandle::ink as *const (), // HANDLE_INK
    crate::fonts::FontHandle::ink_across as *const (), // HANDLE_INK_ACROSS
    crate::fonts::FontHandle::wrap as *const (), // HANDLE_WRAP
    crate::fonts::FontHandle::draw as *const (), // HANDLE_DRAW
    crate::fonts::FontHandle::draw_bolder as *const (), // HANDLE_DRAW_BOLDER
    crate::fonts::FontHandle::draw_toned as *const (), // HANDLE_DRAW_TONED
    paperpaw_text::measure_in as *const (), // TEXT_MEASURE_IN
    paperpaw_text::draw_text_in as *const (), // TEXT_DRAW_TEXT_IN
    paperpaw_text::StoredPack::open_within as *const (), // STORED_OPEN
    paperpaw_text::GlyphCache::new as *const (), // CACHE_NEW
    crate::fonts::Fonts::failed_reads as *const (), // FONTS_FAILED_READS
    crate::fonts::FontHandle::measure_in as *const (), // HANDLE_MEASURE_IN
    crate::fonts::FontHandle::draw_in as *const (), // HANDLE_DRAW_IN
]);

pub(super) static UI: super::Addresses<52> = super::Addresses([
    crate::ui::style::UiStyle::new as *const (), // STYLE_NEW
    crate::ui::style::UiStyle::noted_row as *const (), // STYLE_NOTED_ROW
    crate::ui::style::draw_focus as *const (), // STYLE_DRAW_FOCUS
    crate::ui::style::draw_separator as *const (), // STYLE_DRAW_SEPARATOR
    crate::ui::shows as *const (), // UI_SHOWS
    crate::ui::text::icon as *const (), // TEXT_ICON
    crate::ui::text::draw_icon as *const (), // TEXT_DRAW_ICON
    crate::ui::text::icon_width as *const (), // TEXT_ICON_WIDTH
    crate::ui::text::draw_icon_at as *const (), // TEXT_DRAW_ICON_AT
    crate::ui::text::heading_height as *const (), // TEXT_HEADING_HEIGHT
    crate::ui::text::draw_heading as *const (), // TEXT_DRAW_HEADING
    crate::ui::text::icon_room as *const (), // TEXT_ICON_ROOM
    crate::ui::text::draw_row_icon as *const (), // TEXT_DRAW_ROW_ICON
    crate::ui::text::tr_input as *const (), // TEXT_TR_INPUT
    crate::ui::text::draw_label as *const (), // TEXT_DRAW_LABEL
    crate::ui::text::draw_label_in as *const (), // TEXT_DRAW_LABEL_IN
    crate::ui::text::draw_text as *const (), // TEXT_DRAW_TEXT
    crate::ui::text::put_run as *const (), // TEXT_PUT_RUN
    crate::ui::text::cap_baseline as *const (), // TEXT_CAP_BASELINE
    crate::ui::text::draw_label_on as *const (), // TEXT_DRAW_LABEL_ON
    crate::ui::text::fit_pair as *const (), // TEXT_FIT_PAIR
    crate::ui::text::message_height as *const (), // TEXT_MESSAGE_HEIGHT
    crate::ui::text::draw_message as *const (), // TEXT_DRAW_MESSAGE
    crate::ui::text::note_line_height as *const (), // TEXT_NOTE_LINE_HEIGHT
    crate::ui::text::draw_note_line as *const (), // TEXT_DRAW_NOTE_LINE
    crate::ui::text::fit as *const (), // TEXT_FIT
    crate::ui::text::empty_height as *const (), // TEXT_EMPTY_HEIGHT
    crate::ui::text::draw_empty as *const (), // TEXT_DRAW_EMPTY
    crate::ui::button::draw_button_said as *const (), // BUTTON_DRAW_SAID
    crate::ui::button::button_frame as *const (), // BUTTON_FRAME
    crate::ui::button::button_label as *const (), // BUTTON_LABEL
    crate::ui::button::button_height_for as *const (), // BUTTON_HEIGHT_FOR
    crate::ui::header::Header::new as *const (), // HEADER_NEW
    crate::ui::header::Header::draw as *const (), // HEADER_DRAW
    crate::ui::header::Header::tap as *const (), // HEADER_TAP
    crate::ui::header::draw_arrow as *const (), // HEADER_DRAW_ARROW
    crate::ui::row::Row::height as *const (), // ROW_HEIGHT
    crate::ui::row::Row::draw as *const (), // ROW_DRAW
    crate::ui::row::Row::hit as *const (), // ROW_HIT
    crate::ui::label::Label::height as *const (), // LABEL_HEIGHT
    crate::ui::label::Label::draw_from as *const (), // LABEL_DRAW_FROM
    crate::ui::setting_rows::title_lines as *const (), // SETTING_TITLE_LINES
    crate::ui::setting_rows::noted_row_height as *const (), // SETTING_NOTED_ROW_HEIGHT
    crate::ui::setting_rows::draw_note_below as *const (), // SETTING_DRAW_NOTE_BELOW
    crate::ui::setting_rows::draw_value_row as *const (), // SETTING_DRAW_VALUE_ROW
    crate::ui::list::ListView::set_focus as *const (), // LIST_SET_FOCUS
    crate::ui::list::ListView::focus_change as *const (), // LIST_FOCUS_CHANGE
    crate::ui::list::ListView::paging as *const (), // LIST_PAGING
    crate::ui::list::ListView::move_focus as *const (), // LIST_MOVE_FOCUS
    crate::ui::list::ListView::row_rect as *const (), // LIST_ROW_RECT
    crate::ui::list::ListView::hit as *const (), // LIST_HIT
    crate::ui::list::list_step as *const (), // LIST_STEP
]);

pub(super) static GRAPHICS: super::Addresses<27> = super::Addresses([
    paperpaw_graphics::Canvas::clear as *const (), // CANVAS_CLEAR
    paperpaw_graphics::Canvas::fill as *const (), // CANVAS_FILL
    paperpaw_graphics::Canvas::fill_rect as *const (), // CANVAS_FILL_RECT
    paperpaw_graphics::Canvas::fill_tint as *const (), // CANVAS_FILL_TINT
    paperpaw_graphics::Canvas::fill_shaded as *const (), // CANVAS_FILL_SHADED
    paperpaw_graphics::Canvas::fill_round_shaded as *const (), // CANVAS_FILL_ROUND_SHADED
    paperpaw_graphics::Canvas::stroke as *const (), // CANVAS_STROKE
    paperpaw_graphics::Canvas::fill_round as *const (), // CANVAS_FILL_ROUND
    paperpaw_graphics::Canvas::stroke_round as *const (), // CANVAS_STROKE_ROUND
    paperpaw_graphics::Canvas::stroke_rect as *const (), // CANVAS_STROKE_RECT
    paperpaw_graphics::Canvas::fill_round_rect as *const (), // CANVAS_FILL_ROUND_RECT
    paperpaw_graphics::Canvas::stroke_round_rect as *const (), // CANVAS_STROKE_ROUND_RECT
    paperpaw_graphics::Canvas::draw_line as *const (), // CANVAS_DRAW_LINE
    paperpaw_graphics::Canvas::fill_circle as *const (), // CANVAS_FILL_CIRCLE
    paperpaw_graphics::Canvas::stroke_circle as *const (), // CANVAS_STROKE_CIRCLE
    paperpaw_graphics::Canvas::fill_ring as *const (), // CANVAS_FILL_RING
    paperpaw_graphics::Canvas::draw_gray_row as *const (), // CANVAS_DRAW_GRAY_ROW
    paperpaw_graphics::Canvas::draw_gray_alpha_row as *const (), // CANVAS_DRAW_GRAY_ALPHA_ROW
    paperpaw_graphics::Canvas::draw_rgb_row as *const (), // CANVAS_DRAW_RGB_ROW
    paperpaw_graphics::Canvas::clipped as *const (), // CANVAS_CLIPPED
    paperpaw_graphics::Canvas::describe as *const (), // CANVAS_DESCRIBE
    paperpaw_graphics::Canvas::note_picture as *const (), // CANVAS_NOTE_PICTURE
    paperpaw_graphics::Canvas::keep_diffusion as *const (), // CANVAS_KEEP_DIFFUSION
    paperpaw_graphics::Canvas::take_diffusion as *const (), // CANVAS_TAKE_DIFFUSION
    paperpaw_graphics::round::corner_inset as *const (), // ROUND_CORNER_INSET
    paperpaw_graphics::round::sin_cos as *const (), // ROUND_SIN_COS
    paperpaw_graphics::round::point_at as *const (), // ROUND_POINT_AT
]);

pub(super) static DATA: super::Addresses<30> = super::Addresses([
    paperpaw_i18n::Translator::get as *const (), // I18N_GET
    paperpaw_i18n::Translator::format as *const (), // I18N_FORMAT
    paperpaw_i18n::Translator::plural as *const (), // I18N_PLURAL
    paperpaw_i18n::Catalog::get as *const (), // I18N_CATALOG_GET
    crate::settings::Settings::get as *const (), // SETTINGS_GET
    crate::settings::Settings::scoped_value as *const (), // SETTINGS_SCOPED_VALUE
    crate::settings::Settings::text as *const (), // SETTINGS_TEXT
    paperpaw_json::object as *const (), // JSON_OBJECT
    paperpaw_json::text as *const (), // JSON_TEXT
    paperpaw_json::number as *const (), // JSON_NUMBER
    paperpaw_json::string as *const (), // JSON_STRING
    paperpaw_json::raw_member as *const (), // JSON_RAW_MEMBER
    paperpaw_json::unquote as *const (), // JSON_UNQUOTE
    paperpaw_json::string_array as *const (), // JSON_STRING_ARRAY
    paperpaw_json::stream::Parser::feed as *const (), // JSON_STREAM_FEED
    paperpaw_json::stream::Parser::finish as *const (), // JSON_STREAM_FINISH
    paperpaw_json::stream::read_file as *const (), // JSON_READ_FILE
    paperpaw_config::Parser::next as *const (), // CONFIG_NEXT
    paperpaw_config::Value::as_str as *const (), // CONFIG_AS_STR
    paperpaw_config::Value::as_i64 as *const (), // CONFIG_AS_I64
    paperpaw_config::Value::as_u32 as *const (), // CONFIG_AS_U32
    paperpaw_config::set as *const (), // CONFIG_SET
    paperpaw_config::remove as *const (), // CONFIG_REMOVE
    paperpaw_config::quote as *const (), // CONFIG_QUOTE
    paperpaw_xml::Tokenizer::parse as *const (), // XML_PARSE
    paperpaw_xml::Tokenizer::feed as *const (), // XML_FEED
    paperpaw_xml::Tokenizer::try_feed as *const (), // XML_TRY_FEED
    paperpaw_xml::Tokenizer::finish as *const (), // XML_FINISH
    paperpaw_xml::Attrs::get as *const (), // XML_ATTRS_GET
    paperpaw_xml::decode as *const (), // XML_DECODE
]);

pub(super) static WIDGETS: super::Addresses<4> = super::Addresses([
    crate::widgets::look::widget_look as *const (), // WIDGET_LOOK
    crate::widgets::look::draw_frame as *const (), // WIDGET_DRAW_FRAME
    crate::widgets::look::frame_height as *const (), // WIDGET_FRAME_HEIGHT
    crate::widgets::look::draw_caption as *const (), // WIDGET_DRAW_CAPTION
]);

/// The table of family number `n`, if the firmware shares it.
pub(super) const fn of(n: usize) -> paperpaw_table::Table {
    match n {
        0 => paperpaw_table::Table(TEXT.0.as_ptr()),
        2 => paperpaw_table::Table(UI.0.as_ptr()),
        3 => paperpaw_table::Table(GRAPHICS.0.as_ptr()),
        5 => paperpaw_table::Table(DATA.0.as_ptr()),
        6 => paperpaw_table::Table(WIDGETS.0.as_ptr()),
        _ => paperpaw_table::Table::NONE,
    }
}
