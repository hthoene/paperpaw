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
text 1 LAYOUT_WRAP_KERNED: fn wrap_lines_kerned_in( text: &str, width: &dyn Fn(usize) -> f32, advance: &dyn Fn(usize, char) -> f32, kern: &dyn Fn(usize, char, char) -> f32, hyphenate: Option<Hyphenate<'_>>, options: u32, ) -> Vec<Line>
text 1 LAYOUT_WRAP_BALANCED: fn wrap_lines_balanced_in( text: &str, width: &dyn Fn(usize) -> f32, advance: &dyn Fn(usize, char) -> f32, kern: &dyn Fn(usize, char, char) -> f32, hyphenate: Option<Hyphenate<'_>>, extra_cost: &dyn Fn(f32, usize) -> f32, options: u32, ) -> Vec<Line>
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
ui 1 STYLE_NEW: fn new(env: &Env<'_>) -> Self
ui 1 STYLE_NOTED_ROW: fn noted_row(&self) -> u32
ui 1 STYLE_DRAW_FOCUS: fn draw_focus(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect)
ui 1 STYLE_DRAW_SEPARATOR: fn draw_separator(canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect, indent: u16)
ui 1 UI_SHOWS: fn shows(canvas: &Canvas<'_>, rect: Rect) -> bool
ui 1 TEXT_ICON: fn icon<'f>(env: &Env<'f>, name: &str) -> Option<(FontHandle<'f>, char)>
ui 1 TEXT_DRAW_ICON: fn draw_icon( canvas: &mut Canvas<'_>, env: &Env<'_>, name: &str, size: f32, x: f32, area: Rect, color: Color, ) -> f32
ui 1 TEXT_ICON_WIDTH: fn icon_width(env: &Env<'_>, name: &str, size: f32) -> f32
ui 1 TEXT_DRAW_ICON_AT: fn draw_icon_at( canvas: &mut Canvas<'_>, env: &Env<'_>, name: &str, size: f32, at: (i32, i32), color: Color, ) -> bool
ui 1 TEXT_HEADING_HEIGHT: fn heading_height(env: &Env<'_>, style: &UiStyle) -> u16
ui 1 TEXT_DRAW_HEADING: fn draw_heading( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, rect: Rect, text: &str, )
ui 1 TEXT_ICON_ROOM: fn icon_room(env: &Env<'_>, style: &UiStyle, any: bool) -> u16
ui 1 TEXT_DRAW_ROW_ICON: fn draw_row_icon( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, rect: Rect, room: u16, icon: Option<&str>, ink: Color, ) -> Rect
ui 1 TEXT_TR_INPUT: fn tr_input<'a>(env: &Env<'a>, key: &'a str) -> &'a str
ui 1 TEXT_DRAW_LABEL: fn draw_label( canvas: &mut Canvas<'_>, font: &FontHandle<'_>, size: f32, x: f32, area: Rect, text: &str, color: Color, ) -> f32
ui 1 TEXT_DRAW_LABEL_IN: fn draw_label_in( canvas: &mut Canvas<'_>, font: &FontHandle<'_>, style: &UiStyle, role: TextRole, size: f32, x: f32, area: Rect, text: &str, color: Color, ) -> f32
ui 1 TEXT_DRAW_TEXT: fn draw_text( canvas: &mut Canvas<'_>, font: &FontHandle<'_>, look: Option<(&UiStyle, TextRole)>, size: f32, x: f32, area: Rect, text: &str, color: Color, ) -> f32
ui 1 TEXT_PUT_RUN: fn put_run( canvas: &mut Canvas<'_>, font: &FontHandle<'_>, look: Option<(&UiStyle, TextRole)>, size: f32, x: f32, baseline: i32, area: Rect, text: &str, color: Color, ) -> f32
ui 1 TEXT_CAP_BASELINE: fn cap_baseline(font: &FontHandle<'_>, band: Rect, size: f32) -> i32
ui 1 TEXT_DRAW_LABEL_ON: fn draw_label_on( canvas: &mut Canvas<'_>, font: &FontHandle<'_>, look: Option<(&UiStyle, TextRole)>, size: f32, x: f32, area: Rect, baseline: i32, text: &str, color: Color, ) -> f32
ui 1 TEXT_FIT_PAIR: fn fit_pair( font: &FontHandle<'_>, size: f32, title: &str, value: &str, width: f32, gap: f32, ) -> (String, String, f32)
ui 1 TEXT_MESSAGE_HEIGHT: fn message_height(env: &Env<'_>, style: &UiStyle, width: u16, text: &str) -> u16
ui 1 TEXT_DRAW_MESSAGE: fn draw_message( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect, text: &str, )
ui 1 TEXT_NOTE_LINE_HEIGHT: fn note_line_height(env: &Env<'_>, style: &UiStyle, width: u16, text: &str) -> u16
ui 1 TEXT_DRAW_NOTE_LINE: fn draw_note_line( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect, text: &str, )
ui 1 TEXT_FIT: fn fit(font: &FontHandle<'_>, size: f32, text: &str, width: f32) -> String
ui 1 TEXT_EMPTY_HEIGHT: fn empty_height(env: &Env<'_>, style: &UiStyle, width: u16, title: &str, text: &str) -> u16
ui 1 TEXT_DRAW_EMPTY: fn draw_empty( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, area: Rect, icon: &str, title: &str, text: &str, )
ui 1 BUTTON_DRAW_SAID: fn draw_button_said( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, rect: Rect, label: &str, filled: bool, on: bool, focused: bool, )
ui 1 BUTTON_FRAME: fn button_frame( canvas: &mut Canvas<'_>, style: &UiStyle, rect: Rect, on: bool, focused: bool, ) -> Option<Color>
ui 1 BUTTON_LABEL: fn button_label( canvas: &mut Canvas<'_>, style: &UiStyle, font: &FontHandle<'_>, rect: Rect, label: &str, ink: Color, )
ui 1 BUTTON_HEIGHT_FOR: fn button_height_for(env: &Env<'_>, style: &UiStyle, width: u16, label: &str) -> u16
ui 1 HEADER_NEW: fn new(env: &Env<'_>, style: &UiStyle, back: bool) -> Self
ui 1 HEADER_DRAW: fn draw( &self, ui: &mut super::Ui<'_>, title: &str, note: Option<&str>, page: Option<Paging>, )
ui 1 HEADER_TAP: fn tap( &self, env: &Env<'_>, style: &UiStyle, point: Point, page: Option<Paging>, ) -> Option<crate::Id>
ui 1 HEADER_DRAW_ARROW: fn draw_arrow( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, at: (i32, i32), icon: &str, left: bool, color: Color, )
ui 1 ROW_HEIGHT: fn height(&self, env: &Env<'_>, style: &UiStyle, width: u16) -> u16
ui 1 ROW_DRAW: fn draw(&self, ui: &mut Ui<'_>, rect: Rect)
ui 1 ROW_HIT: fn hit(&self, env: &Env<'_>, style: &UiStyle, rect: Rect, point: Point) -> Option<RowPart>
ui 1 LABEL_HEIGHT: fn height(&self, env: &Env<'_>, style: &UiStyle, width: u16) -> u16
ui 1 LABEL_DRAW_FROM: fn draw_from(&self, ui: &mut Ui<'_>, x: f32, area: Rect) -> f32
ui 1 SETTING_TITLE_LINES: fn title_lines( font: &FontHandle<'_>, style: &UiStyle, width: u16, beside: f32, title: &str, ) -> (Vec<core::ops::Range<usize>>, u16)
ui 1 SETTING_NOTED_ROW_HEIGHT: fn noted_row_height( env: &Env<'_>, style: &UiStyle, width: u16, title: &str, value: &str, note: &str, ) -> u16
ui 1 SETTING_DRAW_NOTE_BELOW: fn draw_note_below( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, rect: Rect, below: u16, focused: bool, note: &str, )
ui 1 SETTING_DRAW_VALUE_ROW: fn draw_value_row( canvas: &mut Canvas<'_>, env: &Env<'_>, style: &UiStyle, rect: Rect, focused: bool, title: &str, value: &str, )
ui 1 LIST_SET_FOCUS: fn set_focus(&mut self, index: usize) -> ListChange
ui 1 LIST_FOCUS_CHANGE: fn focus_change(&self) -> ListChange
ui 1 LIST_PAGING: fn paging(&self, len: usize) -> Option<super::Paging>
ui 1 LIST_MOVE_FOCUS: fn move_focus(&mut self, delta: isize, len: usize) -> ListChange
ui 1 LIST_ROW_RECT: fn row_rect(&self, slot: usize) -> Rect
ui 1 LIST_HIT: fn hit(&self, point: Point, len: usize) -> Option<usize>
ui 1 LIST_STEP: fn list_step(action: &crate::Id, page: isize) -> Option<isize>
data 1 I18N_GET: fn get(&self, key: &str) -> Option<&str>
data 1 I18N_FORMAT: fn format(&self, key: &str, args: &[(&str, Arg<'_>)]) -> String
data 1 I18N_PLURAL: fn plural(&self, key: &str, count: i64, args: &[(&str, Arg<'_>)]) -> String
data 1 I18N_CATALOG_GET: fn get(&self, key: &str) -> Option<&str>
data 1 SETTINGS_GET: fn get(&self, id: &str) -> Option<&SettingValue>
data 1 SETTINGS_SCOPED_VALUE: fn scoped_value(&self, id: &str) -> Option<&SettingValue>
data 1 SETTINGS_TEXT: fn text(&self, id: &str) -> &str
data 1 JSON_OBJECT: fn object(text: &str) -> Vec<(String, Value)>
data 1 JSON_TEXT: fn text<'a>(members: &'a [(String, Value)], key: &str) -> Option<&'a str>
data 1 JSON_NUMBER: fn number(members: &[(String, Value)], key: &str) -> Option<f64>
data 1 JSON_STRING: fn string(text: &str) -> String
data 1 JSON_RAW_MEMBER: fn raw_member<'a>(text: &'a str, key: &str) -> Option<&'a str>
data 1 JSON_UNQUOTE: fn unquote(literal: &str) -> Option<String>
data 1 JSON_STRING_ARRAY: fn string_array(literal: &str) -> Vec<String>
data 1 JSON_STREAM_FEED: fn feed(&mut self, data: &[u8], emit: &mut dyn FnMut(Event<'_>))
data 1 JSON_STREAM_FINISH: fn finish(&mut self, emit: &mut dyn FnMut(Event<'_>))
data 1 JSON_READ_FILE: fn read_file(storage: &dyn Storage, path: &str, emit: &mut dyn FnMut(Event<'_>))
data 1 CONFIG_NEXT: fn next(&mut self) -> Option<Self::Item>
data 1 CONFIG_AS_STR: fn as_str(&self) -> Cow<'a, str>
data 1 CONFIG_AS_I64: fn as_i64(&self) -> Option<i64>
data 1 CONFIG_AS_U32: fn as_u32(&self) -> Option<u32>
data 1 CONFIG_SET: fn set(source: &str, section: &str, key: &str, value: &str) -> String
data 1 CONFIG_REMOVE: fn remove(source: &str, section: &str, key: &str) -> String
data 1 CONFIG_QUOTE: fn quote(value: &str) -> Cow<'_, str>
data 1 XML_PARSE: fn parse(data: &[u8], emit: &mut dyn FnMut(Token<'_>))
data 1 XML_FEED: fn feed(&mut self, data: &[u8], emit: &mut dyn FnMut(Token<'_>))
data 1 XML_TRY_FEED: fn try_feed( &mut self, data: &[u8], emit: &mut dyn FnMut(Token<'_>), ) -> Result<(), TryReserveError>
data 1 XML_FINISH: fn finish(&mut self, emit: &mut dyn FnMut(Token<'_>))
data 1 XML_ATTRS_GET: fn get(&self, name: &str) -> Option<Cow<'a, str>>
data 1 XML_DECODE: fn decode(text: &str) -> Cow<'_, str>
graphics 1 CANVAS_CLEAR: fn clear(&mut self, color: Color)
graphics 1 CANVAS_FILL: fn fill(&mut self, area: Rect, color: Color)
graphics 1 CANVAS_FILL_RECT: fn fill_rect(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color)
graphics 1 CANVAS_FILL_TINT: fn fill_tint(&mut self, area: Rect, color: Color)
graphics 1 CANVAS_FILL_SHADED: fn fill_shaded(&mut self, area: Rect, color: Color)
graphics 1 CANVAS_FILL_ROUND_SHADED: fn fill_round_shaded(&mut self, area: Rect, radius: u32, color: Color)
graphics 1 CANVAS_STROKE: fn stroke(&mut self, area: Rect, thickness: u32, color: Color)
graphics 1 CANVAS_FILL_ROUND: fn fill_round(&mut self, area: Rect, radius: u32, color: Color)
graphics 1 CANVAS_STROKE_ROUND: fn stroke_round(&mut self, area: Rect, thickness: u32, radius: u32, color: Color)
graphics 1 CANVAS_STROKE_RECT: fn stroke_rect( &mut self, x: i32, y: i32, width: u32, height: u32, thickness: u32, color: Color, )
graphics 1 CANVAS_FILL_ROUND_RECT: fn fill_round_rect( &mut self, x: i32, y: i32, width: u32, height: u32, radius: u32, color: Color, )
graphics 1 CANVAS_STROKE_ROUND_RECT: fn stroke_round_rect( &mut self, x: i32, y: i32, width: u32, height: u32, thickness: u32, radius: u32, color: Color, )
graphics 1 CANVAS_DRAW_LINE: fn draw_line(&mut self, from: Point, to: Point, width: u32, color: Color)
graphics 1 CANVAS_FILL_CIRCLE: fn fill_circle(&mut self, center: Point, radius: u32, color: Color)
graphics 1 CANVAS_STROKE_CIRCLE: fn stroke_circle(&mut self, center: Point, radius: u32, thickness: u32, color: Color)
graphics 1 CANVAS_FILL_RING: fn fill_ring( &mut self, center: Point, radius: u32, thickness: u32, from: i32, sweep: u32, color: Color, )
graphics 1 CANVAS_DRAW_GRAY_ROW: fn draw_gray_row(&mut self, x: i32, y: i32, gray: &[u8])
graphics 1 CANVAS_DRAW_GRAY_ALPHA_ROW: fn draw_gray_alpha_row(&mut self, x: i32, y: i32, gray_alpha: &[u8], opacity: u8)
graphics 1 CANVAS_DRAW_RGB_ROW: fn draw_rgb_row(&mut self, x: i32, y: i32, rgb: &[u8])
graphics 1 CANVAS_CLIPPED: fn clipped(&mut self, region: Rect) -> Canvas<'_>
graphics 1 CANVAS_DESCRIBE: fn describe<'c>(&'c mut self, role: Role, label: &'c str, area: Rect) -> Describe<'c>
graphics 1 CANVAS_NOTE_PICTURE: fn note_picture(&mut self, area: Rect)
graphics 1 CANVAS_KEEP_DIFFUSION: fn keep_diffusion(&mut self, key: u64, y: i32, atkinson: crate::diffuse::Atkinson)
graphics 1 CANVAS_TAKE_DIFFUSION: fn take_diffusion(&mut self, key: u64, y: i32) -> Option<crate::diffuse::Atkinson>
graphics 1 ROUND_CORNER_INSET: fn corner_inset(radius: u32, row: u32, height: u32) -> u32
graphics 1 ROUND_SIN_COS: fn sin_cos(degrees: i32) -> (i32, i32)
graphics 1 ROUND_POINT_AT: fn point_at(center: Point, radius: i32, degrees: i32) -> Point
widgets 1 WIDGET_LOOK: fn widget_look(env: &Env<'_>, widget: &str, area: Rect) -> WidgetLook
widgets 1 WIDGET_DRAW_FRAME: fn draw_frame(canvas: &mut Canvas<'_>, env: &Env<'_>, widget: &str, area: Rect) -> WidgetLook
widgets 1 WIDGET_FRAME_HEIGHT: fn frame_height(env: &Env<'_>, widget: &str) -> u16
widgets 1 WIDGET_DRAW_CAPTION: fn draw_caption( canvas: &mut Canvas<'_>, env: &Env<'_>, widget: &str, look: &WidgetLook, text: &str, ) -> Rect
