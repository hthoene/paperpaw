// Made by tools/shared-table.py from shared.list: do not edit.

/// The revision of each family (`shared.list`).
pub const REVISIONS: crate::Revisions = [1, 0, 0, 0, 0, 0, 0, 0];
/// The oldest revision of each family whose plugins still run.
pub const OLDEST: crate::Revisions = [1, 0, 0, 0, 0, 0, 0, 0];
/// Eight hex digits naming the tables of this build, for logs.
pub const HASH: &str = "a424960d";

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
    /// `paperpaw_text::StoredPack::largest_size` (revision 1).
    pub const STORED_LARGEST_SIZE: usize = 20;
    /// `paperpaw_text::StoredPack::has_char` (revision 1).
    pub const STORED_HAS_CHAR: usize = 21;
    /// `paperpaw_text::StoredPack::line_metrics` (revision 1).
    pub const STORED_LINE_METRICS: usize = 22;
    /// `paperpaw_text::StoredPack::advance` (revision 1).
    pub const STORED_ADVANCE: usize = 23;
    /// `paperpaw_text::StoredPack::render` (revision 1).
    pub const STORED_RENDER: usize = 24;
    /// `paperpaw_text::GlyphCache::glyph` (revision 1).
    pub const CACHE_GLYPH: usize = 25;
    /// `paperpaw_text::wrap_lines_kerned_in` (revision 1).
    pub const LAYOUT_WRAP_KERNED: usize = 26;
    /// `paperpaw_text::wrap_lines_balanced_in` (revision 1).
    pub const LAYOUT_WRAP_BALANCED: usize = 27;
    /// `crate::fonts::Fonts::handle` (revision 1).
    pub const FONTS_HANDLE: usize = 28;
    /// `crate::fonts::FontHandle::largest_size` (revision 1).
    pub const HANDLE_LARGEST_SIZE: usize = 29;
    /// `crate::fonts::FontHandle::line_metrics` (revision 1).
    pub const HANDLE_LINE_METRICS: usize = 30;
    /// `crate::fonts::FontHandle::heading_line_height` (revision 1).
    pub const HANDLE_HEADING_LINE_HEIGHT: usize = 31;
    /// `crate::fonts::FontHandle::has_char` (revision 1).
    pub const HANDLE_HAS_CHAR: usize = 32;
    /// `crate::fonts::FontHandle::advance` (revision 1).
    pub const HANDLE_ADVANCE: usize = 33;
    /// `crate::fonts::FontHandle::kern` (revision 1).
    pub const HANDLE_KERN: usize = 34;
    /// `crate::fonts::FontHandle::size_for` (revision 1).
    pub const HANDLE_SIZE_FOR: usize = 35;
    /// `crate::fonts::FontHandle::measure` (revision 1).
    pub const HANDLE_MEASURE: usize = 36;
    /// `crate::fonts::FontHandle::ink` (revision 1).
    pub const HANDLE_INK: usize = 37;
    /// `crate::fonts::FontHandle::ink_across` (revision 1).
    pub const HANDLE_INK_ACROSS: usize = 38;
    /// `crate::fonts::FontHandle::wrap` (revision 1).
    pub const HANDLE_WRAP: usize = 39;
    /// `crate::fonts::FontHandle::draw` (revision 1).
    pub const HANDLE_DRAW: usize = 40;
    /// `crate::fonts::FontHandle::draw_bolder` (revision 1).
    pub const HANDLE_DRAW_BOLDER: usize = 41;
    /// `crate::fonts::FontHandle::draw_toned` (revision 1).
    pub const HANDLE_DRAW_TONED: usize = 42;
    /// How many entries the table has.
    pub const COUNT: usize = 43;
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
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The graphics family's table.
pub mod graphics {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::GRAPHICS;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}

/// The contribution family's table.
pub mod contribution {
    /// The family.
    pub const FAMILY: crate::Family = crate::Family::CONTRIBUTION;
    /// How many entries the table has.
    pub const COUNT: usize = 0;
}
