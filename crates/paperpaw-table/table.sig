# The signature of every shared function, in the order of
# shared.list (tools/shared-table.py). Part of the plugin interface.
text 1 TEXT_MEASURE: fn measure(font: &Font, size: f32, text: &str) -> f32
text 1 TEXT_IS_MARK: fn is_mark(ch: char) -> bool
text 1 TEXT_GLYPH_X: fn glyph_x(pen: f32, left: i16, lone: bool) -> i32
text 1 TEXT_DRAW_TEXT: fn draw_text( canvas: &mut Canvas<'_>, cache: &mut GlyphCache, style: &TextStyle<'_>, x: f32, baseline: i32, text: &str, ) -> f32
text 1 CHARSET_STAND_IN: fn stand_in(ch: char) -> Option<char>
text 1 FONT_FROM_COW: fn from_cow( data: Cow<'static, [u8]>, variations: &[(&str, f32)], ) -> Result<Font, FontError>
text 1 FONT_SIZE_FOR: fn size_for(&self, size: f32, text: &str, display: bool) -> f32
text 1 FONT_LARGEST_SIZE: fn largest_size(&self) -> Option<f32>
text 1 FONT_LINE_METRICS: fn line_metrics(&self, size: f32) -> LineMetrics
text 1 FONT_ADVANCE: fn advance(&self, ch: char, size: f32) -> f32
text 1 FONT_KERN: fn kern(&self, left: char, right: char, size: f32) -> f32
text 1 FONT_HAS_CHAR: fn has_char(&self, ch: char) -> bool
text 1 FONT_SHOWN: fn shown(&self, ch: char) -> char
text 1 PACK_LARGEST_SIZE: fn largest_size(&self) -> Option<f32>
text 1 PACK_LINE_METRICS: fn line_metrics(&self, size: f32) -> LineMetrics
text 1 PACK_HAS_CHAR: fn has_char(&self, ch: char) -> bool
text 1 PACK_SIZE_FOR: fn size_for(&self, size: f32, text: &str, display: bool) -> f32
text 1 PACK_GLYPH: fn glyph(&self, ch: char, size: f32) -> GlyphRef<'_>
text 1 PACK_ADVANCE: fn advance(&self, ch: char, size: f32) -> f32
text 1 PACK_KERN: fn kern(&self, left: char, right: char, size: f32) -> f32
text 1 LAYOUT_WRAP_KERNED: fn wrap_lines_kerned_in( text: &str, width: &dyn Fn(usize) -> f32, advance: &dyn Fn(usize, char) -> f32, kern: &dyn Fn(usize, char, char) -> f32, hyphenate: Option<Hyphenate<'_>>, ) -> Vec<Line>
text 1 LAYOUT_WRAP_BALANCED: fn wrap_lines_balanced_in( text: &str, width: &dyn Fn(usize) -> f32, advance: &dyn Fn(usize, char) -> f32, kern: &dyn Fn(usize, char, char) -> f32, hyphenate: Option<Hyphenate<'_>>, extra_cost: &dyn Fn(f32, usize) -> f32, ) -> Vec<Line>
text 1 FONTS_HANDLE: fn handle(&self, index: usize) -> FontHandle<'_>
text 1 HANDLE_LARGEST_SIZE: fn largest_size(&self) -> Option<f32>
text 1 HANDLE_LINE_METRICS: fn line_metrics(&self, size: f32) -> LineMetrics
text 1 HANDLE_HEADING_LINE_HEIGHT: fn heading_line_height(&self, size: f32) -> f32
text 1 HANDLE_HAS_CHAR: fn has_char(&self, ch: char) -> bool
text 1 HANDLE_ADVANCE: fn advance(&self, size: f32, ch: char) -> f32
text 1 HANDLE_KERN: fn kern(&self, size: f32, left: char, right: char) -> f32
text 1 HANDLE_SIZE_FOR: fn size_for(&self, size: f32, text: &str) -> f32
text 1 HANDLE_MEASURE: fn measure(&self, size: f32, text: &str) -> f32
text 1 HANDLE_INK: fn ink(&self, size: f32, text: &str) -> Option<(i32, i32)>
text 1 HANDLE_INK_ACROSS: fn ink_across(&self, size: f32, text: &str) -> Option<(i32, i32)>
text 1 HANDLE_WRAP: fn wrap(&self, size: f32, text: &str, max_width: f32) -> Vec<Range<usize>>
text 1 HANDLE_DRAW: fn draw( &self, canvas: &mut Canvas<'_>, size: f32, x: f32, baseline: i32, text: &str, color: Color, ) -> f32
text 1 HANDLE_DRAW_BOLDER: fn draw_bolder( &self, canvas: &mut Canvas<'_>, size: f32, x: f32, baseline: i32, text: &str, color: Color, extra: u16, ) -> f32
text 1 HANDLE_DRAW_TONED: fn draw_toned( &self, canvas: &mut Canvas<'_>, size: f32, x: f32, baseline: i32, text: &str, color: Color, extra: u16, tone: paperpaw_text::ToneCurve, ) -> f32
text 1 TEXT_MEASURE_IN: fn measure_in(font: &Font, size: f32, text: &str, around: &Around) -> f32
text 1 TEXT_DRAW_TEXT_IN: fn draw_text_in( canvas: &mut Canvas<'_>, cache: &mut GlyphCache, style: &TextStyle<'_>, x: f32, baseline: i32, text: &str, around: &Around, ) -> f32
text 1 STORED_OPEN: fn open_within( source: Rc<dyn PackSource>, limits: PackLimits, ) -> Result<StoredPack, FontError>
text 1 CACHE_NEW: fn new(capacity: usize) -> GlyphCache
text 1 FONTS_FAILED_READS: fn failed_reads(&self) -> u32
text 1 HANDLE_MEASURE_IN: fn measure_in(&self, size: f32, text: &str, around: &paperpaw_text::Around) -> f32
text 1 HANDLE_DRAW_IN: fn draw_in( &self, canvas: &mut Canvas<'_>, size: f32, x: f32, baseline: i32, text: &str, color: Color, look: (u16, paperpaw_text::ToneCurve), around: &paperpaw_text::Around, ) -> f32
