// Made by tools/shared-table.py from shared.list: do not edit.
#![cfg_attr(rustfmt, rustfmt::skip)]

pub(super) static TEXT: super::Addresses<43> = super::Addresses([
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
    paperpaw_text::StoredPack::largest_size as *const (), // STORED_LARGEST_SIZE
    paperpaw_text::StoredPack::has_char as *const (), // STORED_HAS_CHAR
    paperpaw_text::StoredPack::line_metrics as *const (), // STORED_LINE_METRICS
    paperpaw_text::StoredPack::advance as *const (), // STORED_ADVANCE
    paperpaw_text::StoredPack::render as *const (), // STORED_RENDER
    paperpaw_text::GlyphCache::glyph as *const (), // CACHE_GLYPH
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
]);

/// The table of family number `n`, if the firmware shares it.
pub(super) const fn of(n: usize) -> paperpaw_table::Table {
    match n {
        0 => paperpaw_table::Table(TEXT.0.as_ptr()),
        _ => paperpaw_table::Table::NONE,
    }
}
