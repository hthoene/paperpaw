//! A small CSS subset for EPUB stylesheets.
//!
//! Selectors: `tag`, `.class`, `tag.class` and lists of them; anything
//! more complex (descendants, pseudo-classes, attributes) is skipped, as
//! are `@` rules except `@font-face`. Properties: `text-align`,
//! `direction`, `font-weight`, `font-style`, `font-family`, `display: none`,
//! `text-indent` (also negative, a hanging indent), `font-size`,
//! `line-height`, `margin` and `padding` on all sides, `vertical-align`
//! (`super`, `sub`) and `text-decoration` (`underline`,
//! `line-through`). That covers how books mark centred title pages, bold
//! and italic classes, hidden elements, their embedded fonts, paragraphs
//! without indent or hanging, the space around blocks, block quotes and
//! poems set in, footnote marks and struck words.
//!
//! The parser is fed text in chunks and keeps only the rule being read.

use alloc::collections::TryReserveError;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::{Align, DocumentError, MAX_FAMILIES};
use paperpaw_sdk::lang::Direction;

use crate::fallible::{self, TryPush, TryPushStr};
use crate::package::try_resolve;

/// What a rule or a `style` attribute sets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Props {
    pub align: Option<Align>,
    /// `direction`: `ltr` or `rtl`.
    pub direction: Option<Direction>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    /// `font-variant: small-caps`.
    pub small_caps: Option<bool>,
    pub hidden: Option<bool>,
    /// A font family: its number in [`Rules::families`] plus one; 0 for
    /// the reader's font (a generic family such as `serif`).
    pub family: Option<u8>,
    /// `text-indent`, in hundredths of an em.
    pub indent: Option<i16>,
    /// `margin-top`, `margin-bottom`, `padding-top` and `padding-bottom`,
    /// in hundredths of an em.
    pub margin_top: Option<u16>,
    pub margin_bottom: Option<u16>,
    pub padding_top: Option<u16>,
    pub padding_bottom: Option<u16>,
    /// `font-size`.
    pub size: Option<FontSize>,
    /// `margin-left`, `margin-right`, `padding-left` and `padding-right`,
    /// in hundredths of an em (negative ones as none).
    pub margin_left: Option<u16>,
    pub margin_right: Option<u16>,
    pub padding_left: Option<u16>,
    pub padding_right: Option<u16>,
    /// `line-height`, in percent of the text size.
    pub line_height: Option<u16>,
    /// `vertical-align`: raised, lowered or on the baseline.
    pub vertical: Option<Vertical>,
    /// `text-decoration`: a line under, a line through.
    pub underline: Option<bool>,
    pub strike: Option<bool>,
    /// Floated to the left (`float: left`), or not.
    pub float_left: Option<bool>,
    /// The lines of a drop capital (`initial-letter`), 0 for `normal`.
    pub initial_letter: Option<u8>,
    /// The marker of list items (`list-style-type`).
    pub list_style: Option<ListStyle>,
}

/// The marker of list items (`list-style-type`): the ones books use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListStyle {
    None,
    Disc,
    Circle,
    Square,
    Decimal,
    LowerAlpha,
    UpperAlpha,
    LowerRoman,
    UpperRoman,
}

impl ListStyle {
    /// In the order of their codes in the cache.
    const ALL: [(ListStyle, &'static str); 9] = [
        (ListStyle::None, "none"),
        (ListStyle::Disc, "disc"),
        (ListStyle::Circle, "circle"),
        (ListStyle::Square, "square"),
        (ListStyle::Decimal, "decimal"),
        (ListStyle::LowerAlpha, "lower-alpha"),
        (ListStyle::UpperAlpha, "upper-alpha"),
        (ListStyle::LowerRoman, "lower-roman"),
        (ListStyle::UpperRoman, "upper-roman"),
    ];

    /// A `list-style-type` value; `None` for one not known.
    pub fn parse(value: &str) -> Option<ListStyle> {
        let value = match value {
            "lower-latin" => "lower-alpha",
            "upper-latin" => "upper-alpha",
            value => value,
        };
        Self::ALL
            .iter()
            .find(|(_, name)| *name == value)
            .map(|(style, _)| *style)
    }

    /// As [`ListStyle::parse`], where the case of the value does not matter.
    pub fn parse_nocase(value: &str) -> Option<ListStyle> {
        [
            ("lower-latin", ListStyle::LowerAlpha),
            ("upper-latin", ListStyle::UpperAlpha),
        ]
        .into_iter()
        .chain(Self::ALL.iter().map(|(style, name)| (*name, *style)))
        .find(|(name, _)| name.eq_ignore_ascii_case(value))
        .map(|(_, style)| style)
    }

    fn code(self) -> u8 {
        Self::ALL
            .iter()
            .position(|(style, _)| *style == self)
            .unwrap_or(0) as u8
    }

    fn from_code(code: u8) -> Option<ListStyle> {
        Self::ALL.get(usize::from(code)).map(|(style, _)| *style)
    }
}

/// Where text sits on its line (`vertical-align`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Vertical {
    Baseline,
    Super,
    Sub,
}

impl Vertical {
    fn code(self) -> u8 {
        match self {
            Vertical::Baseline => 0,
            Vertical::Super => 1,
            Vertical::Sub => 2,
        }
    }

    fn from_code(code: u8) -> Option<Vertical> {
        match code {
            0 => Some(Vertical::Baseline),
            1 => Some(Vertical::Super),
            2 => Some(Vertical::Sub),
            _ => None,
        }
    }
}

/// A `font-size`, in percent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FontSize {
    /// Of the parent's size (`em`, `%`, `smaller`, …).
    Relative(u16),
    /// Of body text (`pt`, `px`, `rem`, `large`, …).
    Absolute(u16),
}

impl FontSize {
    /// The size in percent of body text, inside an element of `parent`
    /// percent; limited to what stays readable and fits a page.
    pub fn of(self, parent: u16) -> u16 {
        let size = match self {
            FontSize::Relative(percent) => u32::from(parent) * u32::from(percent) / 100,
            FontSize::Absolute(percent) => u32::from(percent),
        };
        size.clamp(MIN_SIZE, MAX_SIZE) as u16
    }
}

/// Text sizes taken from a book, in percent of body text.
const MIN_SIZE: u32 = 50;
const MAX_SIZE: u32 = 300;

impl Props {
    pub fn is_empty(&self) -> bool {
        *self == Props::default()
    }

    /// The alignment `value` (`text-align`, the `align` attribute) says,
    /// if it is one.
    pub fn set_align(&mut self, value: &str) {
        if let Some(align) = parse_align(value) {
            self.align = Some(align);
        }
    }

    /// `other` overrides what it sets.
    pub fn apply(&mut self, other: &Props) {
        if other.align.is_some() {
            self.align = other.align;
        }
        self.direction = other.direction.or(self.direction);
        self.bold = other.bold.or(self.bold);
        self.italic = other.italic.or(self.italic);
        self.small_caps = other.small_caps.or(self.small_caps);
        self.hidden = other.hidden.or(self.hidden);
        self.family = other.family.or(self.family);
        self.indent = other.indent.or(self.indent);
        self.margin_top = other.margin_top.or(self.margin_top);
        self.margin_bottom = other.margin_bottom.or(self.margin_bottom);
        self.padding_top = other.padding_top.or(self.padding_top);
        self.padding_bottom = other.padding_bottom.or(self.padding_bottom);
        self.size = other.size.or(self.size);
        self.margin_left = other.margin_left.or(self.margin_left);
        self.margin_right = other.margin_right.or(self.margin_right);
        self.padding_left = other.padding_left.or(self.padding_left);
        self.padding_right = other.padding_right.or(self.padding_right);
        self.line_height = other.line_height.or(self.line_height);
        self.vertical = other.vertical.or(self.vertical);
        self.underline = other.underline.or(self.underline);
        self.strike = other.strike.or(self.strike);
        self.float_left = other.float_left.or(self.float_left);
        self.initial_letter = other.initial_letter.or(self.initial_letter);
        self.list_style = other.list_style.or(self.list_style);
    }

    /// The margin and padding at the start and the end of the lines, in
    /// hundredths of an em.
    pub fn sides(&self) -> (u16, u16) {
        (
            sum(self.margin_left, self.padding_left).unwrap_or(0),
            sum(self.margin_right, self.padding_right).unwrap_or(0),
        )
    }

    /// The `margin` or `padding` shorthand of one to four values.
    fn set_box(&mut self, margin: bool, value: &str) {
        // One to four values: top, right, bottom, left; the
        // bottom as the top and the left as the right where
        // they are not given, the right as the top.
        let mut words = value.split_whitespace();
        let values: [Option<&str>; 4] = core::array::from_fn(|_| words.next());
        let top = values[0].and_then(space);
        let bottom = values[2].or(values[0]).and_then(space);
        let right = values[1].or(values[0]);
        let left = values[3].or(right).and_then(side);
        let right = right.and_then(side);
        if margin {
            self.margin_top = top.or(self.margin_top);
            self.margin_bottom = bottom.or(self.margin_bottom);
            self.margin_left = left.or(self.margin_left);
            self.margin_right = right.or(self.margin_right);
        } else {
            self.padding_top = top.or(self.padding_top);
            self.padding_bottom = bottom.or(self.padding_bottom);
            self.padding_left = left.or(self.padding_left);
            self.padding_right = right.or(self.padding_right);
        }
    }

    /// Reads declarations such as `text-align: center; font-weight: bold`;
    /// font families are looked up in (and added to) `families`.
    pub fn parse_with(
        declarations: &str,
        families: &mut Vec<String>,
    ) -> Result<Props, DocumentError> {
        let mut props = Props::default();
        for declaration in declarations.split(';') {
            let Some((property, value)) = declaration.split_once(':') else {
                continue;
            };
            // A property is a short word: one longer is none we know.
            let mut name = [0u8; 32];
            let property = property.trim();
            let Some(name) = name.get_mut(..property.len()) else {
                continue;
            };
            name.copy_from_slice(property.as_bytes());
            name.make_ascii_lowercase();
            let Ok(lowered) = core::str::from_utf8(name) else {
                continue;
            };
            let value = fallible::lowercase(value.trim().trim_end_matches("!important").trim())?;
            match lowered {
                "text-align" => props.set_align(&value),
                "direction" => props.direction = parse_direction(&value).or(props.direction),
                "font-weight" => {
                    props.bold = match value.as_str() {
                        "bold" | "bolder" | "600" | "700" | "800" | "900" => Some(true),
                        "normal" | "lighter" | "100" | "200" | "300" | "400" | "500" => Some(false),
                        _ => props.bold,
                    }
                }
                "font-style" => {
                    props.italic = match value.as_str() {
                        "italic" | "oblique" => Some(true),
                        "normal" => Some(false),
                        _ => props.italic,
                    }
                }
                "font-variant" | "font-variant-caps" => {
                    props.small_caps =
                        if value.contains("small-caps") || value.contains("petite-caps") {
                            Some(true)
                        } else if value == "normal" || value == "none" {
                            Some(false)
                        } else {
                            props.small_caps
                        }
                }
                "display" => {
                    props.hidden = Some(value == "none");
                }
                "font-family" => props.family = Some(family_number(&value, families)?),
                "font-size" => props.size = font_size(&value).or(props.size),
                "text-indent" => {
                    // Negative: a hanging first line (into the margin).
                    let indent = length(&value).map(|l| l.clamp(-MAX_INDENT, MAX_INDENT) as i16);
                    props.indent = indent.or(props.indent);
                }
                "margin-left" => props.margin_left = side(&value).or(props.margin_left),
                "margin-right" => props.margin_right = side(&value).or(props.margin_right),
                "padding-left" => props.padding_left = side(&value).or(props.padding_left),
                "padding-right" => props.padding_right = side(&value).or(props.padding_right),
                "line-height" => props.line_height = line_height(&value).or(props.line_height),
                "vertical-align" => {
                    props.vertical = match value.as_str() {
                        "super" | "text-top" => Some(Vertical::Super),
                        "sub" | "text-bottom" => Some(Vertical::Sub),
                        "baseline" => Some(Vertical::Baseline),
                        _ => props.vertical,
                    }
                }
                "text-decoration" | "text-decoration-line" => {
                    (props.underline, props.strike) =
                        decoration(&value, (props.underline, props.strike));
                }
                "float" => props.float_left = Some(value == "left"),
                "initial-letter" => {
                    props.initial_letter = initial_letter(&value).or(props.initial_letter);
                }
                "list-style-type" | "list-style" => {
                    props.list_style = list_style(&value).or(props.list_style);
                }
                "margin-top" => props.margin_top = space(&value).or(props.margin_top),
                "margin-bottom" => props.margin_bottom = space(&value).or(props.margin_bottom),
                "padding-top" => props.padding_top = space(&value).or(props.padding_top),
                "padding-bottom" => {
                    props.padding_bottom = space(&value).or(props.padding_bottom);
                }
                "margin" | "padding" => {
                    props.set_box(property.eq_ignore_ascii_case("margin"), &value)
                }
                _ => {}
            }
        }
        Ok(props)
    }

    /// Compact text form for the cache, e.g. `a2 b1 i0 h1`.
    pub fn encode(&self, out: &mut String) -> Result<(), DocumentError> {
        if let Some(align) = self.align {
            fallible::try_write!(out, " a{}", align.bits())?;
        }
        match self.direction {
            Some(Direction::Ltr) => out.try_push_str(" d1")?,
            Some(Direction::Rtl) => out.try_push_str(" d2")?,
            _ => {}
        }
        if let Some(family) = self.family {
            fallible::try_write!(out, " f{family}")?;
        }
        if let Some(indent) = self.indent {
            fallible::try_write!(out, " t{indent}")?;
        }
        match self.size {
            Some(FontSize::Relative(percent)) => fallible::try_write!(out, " r{percent}")?,
            Some(FontSize::Absolute(percent)) => fallible::try_write!(out, " z{percent}")?,
            None => {}
        }
        for (key, value) in [
            ('m', self.margin_top),
            ('n', self.margin_bottom),
            ('p', self.padding_top),
            ('q', self.padding_bottom),
            ('L', self.margin_left),
            ('R', self.margin_right),
            ('P', self.padding_left),
            ('Q', self.padding_right),
            ('H', self.line_height),
            ('v', self.vertical.map(|v| u16::from(v.code()))),
            ('I', self.initial_letter.map(u16::from)),
            ('y', self.list_style.map(|l| u16::from(l.code()))),
        ] {
            if let Some(value) = value {
                fallible::try_write!(out, " {key}{value}")?;
            }
        }
        for (key, value) in [
            ('b', self.bold),
            ('i', self.italic),
            ('c', self.small_caps),
            ('h', self.hidden),
            ('u', self.underline),
            ('s', self.strike),
            ('F', self.float_left),
        ] {
            if let Some(value) = value {
                out.try_push(' ')?;
                out.try_push(key)?;
                out.try_push(if value { '1' } else { '0' })?;
            }
        }
        Ok(())
    }

    pub fn decode(text: &str) -> Props {
        let mut props = Props::default();
        for item in text.split_whitespace() {
            let mut chars = item.chars();
            let (Some(key), Some(value)) = (chars.next(), chars.next()) else {
                continue;
            };
            let flag = value == '1';
            match key {
                'a' => props.align = value.to_digit(10).map(|d| Align::from_bits(d as u8)),
                'd' => {
                    props.direction = match value {
                        '1' => Some(Direction::Ltr),
                        '2' => Some(Direction::Rtl),
                        _ => None,
                    }
                }
                'b' => props.bold = Some(flag),
                'i' => props.italic = Some(flag),
                'c' => props.small_caps = Some(flag),
                'h' => props.hidden = Some(flag),
                'f' => props.family = item[1..].parse().ok(),
                't' => props.indent = item[1..].parse().ok(),
                'm' => props.margin_top = item[1..].parse().ok(),
                'n' => props.margin_bottom = item[1..].parse().ok(),
                'p' => props.padding_top = item[1..].parse().ok(),
                'q' => props.padding_bottom = item[1..].parse().ok(),
                'L' => props.margin_left = item[1..].parse().ok(),
                'R' => props.margin_right = item[1..].parse().ok(),
                'P' => props.padding_left = item[1..].parse().ok(),
                'Q' => props.padding_right = item[1..].parse().ok(),
                'H' => props.line_height = item[1..].parse().ok(),
                'v' => props.vertical = item[1..].parse().ok().and_then(Vertical::from_code),
                'u' => props.underline = Some(flag),
                's' => props.strike = Some(flag),
                'F' => props.float_left = Some(flag),
                'I' => props.initial_letter = item[1..].parse().ok(),
                'y' => props.list_style = item[1..].parse().ok().and_then(ListStyle::from_code),
                'r' => props.size = item[1..].parse().ok().map(FontSize::Relative),
                'z' => props.size = item[1..].parse().ok().map(FontSize::Absolute),
                _ => {}
            }
        }
        props
    }
}

/// The largest indent and space taken from a book, in hundredths of an
/// em, so a stray value cannot push text off the page.
const MAX_INDENT: i32 = 500;
/// The largest margin at a side, in hundredths of an em (the layout also
/// keeps it to a quarter of a line).
const MAX_SIDE: i32 = 1000;

/// A margin or padding at a side, in hundredths of an em: a percentage
/// of the width taken as a quarter of an em per percent (a line holds
/// some 25 to 40 ems); negative ones as none.
fn side(value: &str) -> Option<u16> {
    let value = value.trim();
    let side = match value.strip_suffix('%') {
        Some(percent) => percent
            .trim()
            .parse::<f32>()
            .ok()
            .map(|p| (p * 25.0) as i32),
        None if value == "auto" => Some(0),
        None => length(value),
    };
    side.map(|s| s.clamp(0, MAX_SIDE) as u16)
}

/// A `line-height`, in percent of the text size: a number (`1.5`), a
/// percentage or a length; `normal` and what is not understood as none.
fn line_height(value: &str) -> Option<u16> {
    let value = value.trim();
    let percent = if let Some(percent) = value.strip_suffix('%') {
        percent.trim().parse::<f32>().ok()?
    } else if let Ok(number) = value.parse::<f32>() {
        number * 100.0
    } else {
        length(value)? as f32
    };
    (percent > 0.0).then(|| (percent as i32).clamp(50, 400) as u16)
}
const MAX_SPACE: i32 = 800;

/// A CSS length in hundredths of an em: `em` and `rem` as they are, `pt`
/// and `px` at the usual 12 pt and 16 px to the em, `0` without a unit.
/// `None` for other units (e.g. `%`) and keywords such as `auto`.
pub(crate) fn length(value: &str) -> Option<i32> {
    let value = value.trim();
    let split = value
        .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: f32 = number.parse().ok()?;
    let em = match unit.trim() {
        "em" | "rem" => number,
        "ex" => number / 2.0,
        "pt" => number / 12.0,
        "px" => number / 16.0,
        "" if number == 0.0 => 0.0,
        _ => return None,
    };
    // Rounded (no_std has no `f32::round`).
    let half = if em < 0.0 { -0.5 } else { 0.5 };
    Some((em * 100.0 + half) as i32)
}

/// A `font-size` value; `None` for what is not understood.
/// Lines under and through text as `text-decoration` sets them, over
/// `(underline, strike)` as they were.
fn decoration(
    value: &str,
    (mut underline, mut strike): (Option<bool>, Option<bool>),
) -> (Option<bool>, Option<bool>) {
    if value.contains("none") {
        (underline, strike) = (Some(false), Some(false));
    }
    if value.contains("underline") {
        underline = Some(true);
    }
    if value.contains("line-through") {
        strike = Some(true);
    }
    (underline, strike)
}

/// The marker type of `list-style-type`, or of the `list-style`
/// shorthand among its position and picture.
fn list_style(value: &str) -> Option<ListStyle> {
    value.split_whitespace().find_map(ListStyle::parse)
}

/// The lines of a drop capital CSS `initial-letter` asks for (a sink
/// after them is left out); 0 for `normal`.
fn initial_letter(value: &str) -> Option<u8> {
    if value == "normal" {
        return Some(0);
    }
    let lines: f32 = value.split_whitespace().next()?.parse().ok()?;
    Some(lines.clamp(0.0, 9.0) as u8)
}

fn font_size(value: &str) -> Option<FontSize> {
    let keyword = match value.trim() {
        "xx-small" => Some(FontSize::Absolute(60)),
        "x-small" => Some(FontSize::Absolute(75)),
        "small" => Some(FontSize::Absolute(89)),
        "medium" => Some(FontSize::Absolute(100)),
        "large" => Some(FontSize::Absolute(120)),
        "x-large" => Some(FontSize::Absolute(150)),
        "xx-large" => Some(FontSize::Absolute(200)),
        "smaller" => Some(FontSize::Relative(83)),
        "larger" => Some(FontSize::Relative(120)),
        _ => None,
    };
    if keyword.is_some() {
        return keyword;
    }
    let value = value.trim();
    if let Some(percent) = value.strip_suffix('%') {
        let percent: f32 = percent.trim().parse().ok()?;
        return (percent > 0.0).then_some(FontSize::Relative((percent + 0.5) as u16));
    }
    let percent = u16::try_from(length(value)?).ok().filter(|&p| p > 0)?;
    // Lengths in `em` are relative to the parent; `rem`, `pt` and `px` to
    // body text.
    Some(if value.ends_with("em") && !value.ends_with("rem") {
        FontSize::Relative(percent)
    } else {
        FontSize::Absolute(percent)
    })
}

/// A vertical margin or padding: not negative, limited.
fn space(value: &str) -> Option<u16> {
    length(value).map(|l| l.clamp(0, MAX_SPACE) as u16)
}

/// The sum of two spaces, `None` if neither is set.
pub(crate) fn sum(a: Option<u16>, b: Option<u16>) -> Option<u16> {
    match (a, b) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0).saturating_add(b.unwrap_or(0))),
    }
}

/// The number of the first family in a `font-family` list: its place in
/// `families` plus one, added if new; 0 for a generic family.
fn family_number(value: &str, families: &mut Vec<String>) -> Result<u8, DocumentError> {
    let mut first = None;
    for part in value.split(',') {
        let name = family_name(part)?;
        if !name.is_empty() {
            first = Some(name);
            break;
        }
    }
    let Some(name) = first else {
        return Ok(0);
    };
    const GENERIC: [&str; 9] = [
        "serif",
        "sans-serif",
        "monospace",
        "cursive",
        "fantasy",
        "system-ui",
        "inherit",
        "initial",
        "unset",
    ];
    if GENERIC.contains(&name.as_str()) {
        return Ok(0);
    }
    Ok(match families.iter().position(|f| *f == name) {
        Some(at) => at as u8 + 1,
        None if families.len() < usize::from(MAX_FAMILIES) => {
            families.try_push(name)?;
            families.len() as u8
        }
        None => 0,
    })
}

/// A family name as compared: without quotes, in lower case, white space
/// and control characters one space each (so the cache, a line per family,
/// keeps it whole).
fn family_name(text: &str) -> Result<String, DocumentError> {
    let text = text.trim().trim_matches(|c| c == '"' || c == '\'');
    let mut out = fallible::string_with(text.len())?;
    for word in text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|word| !word.is_empty())
    {
        if !out.is_empty() {
            out.try_push(' ')?;
        }
        out.try_push_str(word)?;
    }
    out.make_ascii_lowercase();
    Ok(out)
}

/// `ltr` or `rtl`, as `direction` and the `dir` attribute say; `auto`
/// (the attribute's) as from the text.
pub(crate) fn parse_direction(value: &str) -> Option<Direction> {
    let value = value.trim();
    [
        ("ltr", Direction::Ltr),
        ("rtl", Direction::Rtl),
        ("auto", Direction::Auto),
    ]
    .into_iter()
    .find(|(word, _)| word.eq_ignore_ascii_case(value))
    .map(|(_, direction)| direction)
}

pub(crate) fn parse_align(value: &str) -> Option<Align> {
    let value = value.trim();
    [
        ("center", Align::Center),
        ("right", Align::Right),
        ("left", Align::Left),
        ("end", Align::End),
        ("start", Align::Start),
        ("justify", Align::Justify),
    ]
    .into_iter()
    .find(|(word, _)| word.eq_ignore_ascii_case(value))
    .map(|(_, align)| align)
}

/// A simple selector: element name and/or one class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selector {
    pub tag: Option<String>,
    pub class: Option<String>,
}

impl Selector {
    fn parse(text: &str) -> Result<Option<Selector>, DocumentError> {
        let text = text.trim();
        if text.is_empty()
            || text
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '>' | '+' | '~' | ':' | '[' | '#' | '*'))
        {
            return Ok(None);
        }
        let (tag, class) = match text.split_once('.') {
            Some((tag, class)) if !class.contains('.') => (tag, Some(class)),
            Some(_) => return Ok(None),
            None => (text, None),
        };
        Ok(Some(Selector {
            tag: (!tag.is_empty())
                .then(|| fallible::lowercase(tag))
                .transpose()?,
            class: class.map(fallible::string).transpose()?,
        }))
    }

    pub fn matches(&self, tag: &str, classes: &str) -> bool {
        self.tag.as_deref().is_none_or(|t| t == tag)
            && self
                .class
                .as_deref()
                .is_none_or(|c| classes.split_ascii_whitespace().any(|k| k == c))
    }

    /// Class selectors outrank element selectors.
    pub fn specificity(&self) -> u8 {
        u8::from(self.class.is_some()) * 2 + u8::from(self.tag.is_some())
    }
}

/// Stylesheet rules in source order, with the font families they name
/// and the fonts the stylesheets embed.
#[derive(Debug, Default)]
pub(crate) struct Rules {
    pub rules: Vec<(Selector, Props)>,
    /// Font family names in the order first named.
    pub families: Vec<String>,
    pub faces: Vec<FontFace>,
    /// One's own rules (see [`Rules::add_own`]): after all others.
    own: Vec<(Selector, Props)>,
    /// A rule or font was dropped as the heap had no room for it.
    pub out_of_memory: bool,
    /// The last elements' properties (see [`Rules::props_remembered`]).
    memo: Vec<Remembered>,
    /// The entry of `memo` replaced next once it is full.
    memo_next: usize,
}

/// What [`Rules::props`] found for an element: its tag and class
/// attribute side by side in `key`, the first `tag` bytes the tag.
#[derive(Debug)]
struct Remembered {
    key: [u8; MEMO_KEY],
    tag: u8,
    len: u8,
    props: Props,
}

impl Remembered {
    fn is(&self, tag: &str, classes: &str) -> bool {
        let (tag_len, len) = (usize::from(self.tag), usize::from(self.len));
        tag_len == tag.len()
            && len == tag.len() + classes.len()
            && self.key[..tag_len] == *tag.as_bytes()
            && self.key[tag_len..len] == *classes.as_bytes()
    }
}

/// Elements remembered at most: a chapter has few distinct ones.
const MEMO: usize = 8;
/// Longest tag and class attribute together that are remembered.
const MEMO_KEY: usize = 48;

/// A font embedded with `@font-face`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FontFace {
    /// As in [`Props::family`].
    pub family: u8,
    pub bold: bool,
    pub italic: bool,
    /// The font file in the archive.
    pub path: String,
}

/// Rules read beyond this are dropped (memory bound on small devices).
const MAX_RULES: usize = 512;
/// Embedded fonts kept at most, so their list stays readable.
const MAX_FACES: usize = 64;

impl Rules {
    /// What the rules set for an element, by specificity, then order.
    pub fn props(&self, tag: &str, classes: &str) -> Props {
        let mut props = Props::default();
        // The book's rules, then one's own over them.
        for rules in [&self.rules, &self.own] {
            for level in 0..=3 {
                for (selector, rule) in rules {
                    if selector.specificity() == level && selector.matches(tag, classes) {
                        props.apply(rule);
                    }
                }
            }
        }
        props
    }

    /// As [`Rules::props`], remembered for the last few elements: in a
    /// chapter the same tags and classes come again and again. Adding
    /// rules forgets them.
    pub fn props_remembered(&mut self, tag: &str, classes: &str) -> Props {
        if let Some(known) = self.memo.iter().find(|m| m.is(tag, classes)) {
            return known.props;
        }
        let props = self.props(tag, classes);
        let len = tag.len() + classes.len();
        let (Ok(tag_len), Ok(len_byte)) = (u8::try_from(tag.len()), u8::try_from(len)) else {
            return props;
        };
        if len > MEMO_KEY {
            return props;
        }
        let mut key = [0; MEMO_KEY];
        key[..tag.len()].copy_from_slice(tag.as_bytes());
        key[tag.len()..len].copy_from_slice(classes.as_bytes());
        let entry = Remembered {
            key,
            tag: tag_len,
            len: len_byte,
            props,
        };
        if self.memo.len() < MEMO {
            if self.memo.try_reserve_exact(MEMO - self.memo.len()).is_ok() {
                self.memo.push(entry);
            }
        } else if let Some(slot) = self.memo.get_mut(self.memo_next) {
            *slot = entry;
            self.memo_next = (self.memo_next + 1) % MEMO;
        }
        props
    }

    /// Adds one's own stylesheet `css`: its rules apply after all of the
    /// book's, also those it adds later (`<style>` in a chapter).
    pub fn add_own(&mut self, css: &str) {
        let start = self.rules.len();
        match Parser::new("") {
            Ok(mut parser) => parser.feed(css, self),
            Err(_) => self.out_of_memory = true,
        }
        // Moved one by one: the room is asked for first, and a rule that finds
        // none is dropped (see `out_of_memory`).
        if self
            .own
            .try_reserve(self.rules.len().saturating_sub(start))
            .is_err()
        {
            self.out_of_memory = true;
            self.rules.truncate(start);
        }
        for rule in self.rules.drain(start..) {
            self.own.push(rule);
        }
        self.memo.clear();
    }

    /// Parses a `style` attribute.
    pub fn parse_style(&mut self, declarations: &str) -> Props {
        // Where there is no room for it: none, and the chapter ends as
        // out of memory.
        Props::parse_with(declarations, &mut self.families).unwrap_or_else(|_| {
            self.out_of_memory = true;
            Props::default()
        })
    }

    /// Adds an `@font-face` rule of a stylesheet in folder `base`: its
    /// OpenType, TrueType or WOFF file (see [`font_url`]); [`MAX_FACES`]
    /// at most.
    fn add_face(&mut self, declarations: &str, base: &str) {
        if self.add_face_checked(declarations, base).is_err() {
            self.out_of_memory = true;
        }
    }

    fn add_face_checked(&mut self, declarations: &str, base: &str) -> Result<(), DocumentError> {
        if self.faces.len() >= MAX_FACES {
            return Ok(());
        }
        let mut name = String::new();
        let mut src = None;
        let (mut bold, mut italic) = (false, false);
        for declaration in declarations.split(';') {
            let Some((property, value)) = declaration.split_once(':') else {
                continue;
            };
            let value = value.trim();
            let property = property.trim();
            if property.eq_ignore_ascii_case("font-family") {
                name = family_name(value)?;
            } else if property.eq_ignore_ascii_case("font-weight") {
                bold = value.eq_ignore_ascii_case("bold")
                    || value.parse::<u32>().is_ok_and(|w| w >= 600);
            } else if property.eq_ignore_ascii_case("font-style") {
                italic =
                    value.eq_ignore_ascii_case("italic") || value.eq_ignore_ascii_case("oblique");
            } else if property.eq_ignore_ascii_case("src") {
                src = font_url(value)?;
            }
        }
        let Some(url) = src else {
            return Ok(());
        };
        if name.is_empty() {
            return Ok(());
        }
        let family = family_number(&name, &mut self.families)?;
        if family == 0 {
            return Ok(());
        }
        let (path, _) = try_resolve(base, &url)?;
        fallible::reserve(&mut self.faces, 1)?;
        self.faces.push(FontFace {
            family,
            bold,
            italic,
            path,
        });
        Ok(())
    }

    fn add(&mut self, selectors: &str, declarations: &str) {
        let Ok(props) = Props::parse_with(declarations, &mut self.families) else {
            self.out_of_memory = true;
            return;
        };
        if props.is_empty() {
            return;
        }
        for selector in selectors.split(',').map(Selector::parse) {
            let selector = match selector {
                Ok(Some(selector)) => selector,
                Ok(None) => continue,
                Err(_) => {
                    self.out_of_memory = true;
                    break;
                }
            };
            if self.rules.len() >= MAX_RULES {
                break;
            }
            if self.rules.try_reserve(1).is_err() {
                self.out_of_memory = true;
                break;
            }
            self.rules.push((selector, props));
            self.memo.clear();
        }
    }

    /// Cache form: one `@family<TAB>name` line per family, then one
    /// `selector<TAB>props` line per rule (embedded fonts are kept apart,
    /// see `package`).
    #[cfg(test)]
    pub fn encode(&self) -> String {
        let mut out = String::new();
        let _ = self.encode_lines(&mut |line| out.push_str(line));
        out
    }

    /// The cache form (see `encode`) a line at a time, each with its line
    /// end, so that it is never all in memory.
    pub fn encode_lines(&self, out: &mut dyn FnMut(&str)) -> Result<(), DocumentError> {
        let mut line = String::new();
        for family in &self.families {
            line.clear();
            line.try_push_str("@family\t")?;
            line.try_push_str(family)?;
            line.try_push('\n')?;
            out(&line);
        }
        for (selector, props) in &self.rules {
            line.clear();
            if let Some(tag) = &selector.tag {
                line.try_push_str(tag)?;
            }
            if let Some(class) = &selector.class {
                line.try_push('.')?;
                line.try_push_str(class)?;
            }
            line.try_push('\t')?;
            props.encode(&mut line)?;
            line.try_push('\n')?;
            out(&line);
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn decode(text: &str) -> Rules {
        Self::try_decode(text).unwrap_or_default()
    }

    /// The rules of their cache form; an error where the heap has no room
    /// for them. The list of rules is made once, as long as it must be.
    pub fn try_decode(text: &str) -> Result<Rules, TryReserveError> {
        let mut families = Vec::new();
        let mut rules = Vec::new();
        let lines = text.lines().filter(|l| !l.starts_with("@family\t"));
        rules.try_reserve_exact(lines.count())?;
        for line in text.lines() {
            let Some((selector, props)) = line.split_once('\t') else {
                continue;
            };
            if selector == "@family" {
                families.try_reserve(1)?;
                families.push(String::from(props));
            } else if let Ok(Some(selector)) = Selector::parse(selector) {
                rules.push((selector, Props::decode(props)));
            }
        }
        Ok(Rules {
            rules,
            families,
            ..Rules::default()
        })
    }
}

/// Reads a stylesheet in chunks.
#[derive(Debug, Default)]
pub(crate) struct Parser {
    /// Text of the current selector list or declaration block.
    buffer: String,
    selectors: String,
    in_block: bool,
    in_comment: bool,
    /// Depth inside an `@` rule's braces, which are skipped.
    skip_depth: u32,
    /// Inside an `@font-face` block.
    in_face: bool,
    /// Inside a quoted string, and its quote: braces, semicolons and
    /// comments in it are text.
    quote: Option<char>,
    previous: char,
    /// Folder of the stylesheet, for font URLs.
    base: String,
}

/// Longest selector list or declaration block that is kept.
const MAX_PART: usize = 2048;

impl Parser {
    /// A parser for a stylesheet in folder `base` (ending in `/`).
    pub fn new(base: &str) -> Result<Parser, DocumentError> {
        Ok(Parser {
            base: fallible::string(base)?,
            ..Parser::default()
        })
    }

    pub fn feed(&mut self, text: &str, rules: &mut Rules) {
        // A buffer that finds no room ends the reading: what was read stays,
        // and the chapter ends as out of memory (see `Rules::out_of_memory`).
        if self.read(text, rules).is_err() {
            rules.out_of_memory = true;
            self.buffer.clear();
        }
    }

    fn read(&mut self, text: &str, rules: &mut Rules) -> Result<(), DocumentError> {
        for c in text.chars() {
            let previous = core::mem::replace(&mut self.previous, c);
            if self.in_comment {
                if previous == '*' && c == '/' {
                    self.in_comment = false;
                    self.previous = ' ';
                }
                continue;
            }
            // A line break not escaped ends a string left open, as in CSS,
            // so the rules after it are read as rules.
            if self.quote.is_some() && matches!(c, '\n' | '\r' | '\x0c') && previous != '\\' {
                self.quote = None;
            }
            if let Some(quote) = self.quote {
                if c == quote && previous != '\\' {
                    self.quote = None;
                }
                if self.skip_depth == 0 && self.buffer.len() < MAX_PART {
                    self.buffer.try_push(c)?;
                }
                // An escaped backslash escapes nothing after it.
                if previous == '\\' && c == '\\' {
                    self.previous = ' ';
                }
                continue;
            }
            if previous == '/' && c == '*' {
                self.buffer.pop();
                self.in_comment = true;
                // Its star does not end it (`/*/`).
                self.previous = ' ';
                continue;
            }
            if matches!(c, '"' | '\'') {
                self.quote = Some(c);
            }
            // White space alone does not fill what is kept of a part.
            if c.is_whitespace() {
                if self.buffer.ends_with(' ') || self.buffer.trim_start().is_empty() {
                    continue;
                }
                if self.skip_depth == 0 && self.buffer.len() < MAX_PART {
                    self.buffer.try_push(' ')?;
                }
                continue;
            }
            if self.skip_depth > 0 {
                match c {
                    '{' => self.skip_depth += 1,
                    '}' => self.skip_depth -= 1,
                    _ => {}
                }
                continue;
            }
            match c {
                '{' if !self.in_block => {
                    let head = self.buffer.trim_start();
                    let face = head
                        .get(.."@font-face".len())
                        .is_some_and(|word| word.eq_ignore_ascii_case("@font-face"));
                    if face {
                        self.in_face = true;
                        self.in_block = true;
                        self.buffer.clear();
                    } else if head.starts_with('@') {
                        self.skip_depth = 1;
                        self.buffer.clear();
                    } else {
                        self.selectors = core::mem::take(&mut self.buffer);
                        self.in_block = true;
                    }
                }
                '}' if self.in_block && self.in_face => {
                    rules.add_face(&self.buffer, &self.base);
                    self.buffer.clear();
                    self.in_face = false;
                    self.in_block = false;
                }
                '}' if self.in_block => {
                    rules.add(&self.selectors, &self.buffer);
                    self.buffer.clear();
                    self.selectors.clear();
                    self.in_block = false;
                }
                ';' if !self.in_block && self.buffer.trim_start().starts_with('@') => {
                    // `@import …;`, `@charset …;`
                    self.buffer.clear();
                }
                c if self.buffer.len() < MAX_PART => self.buffer.try_push(c)?,
                _ => {}
            }
        }
        Ok(())
    }
}

/// The first OpenType, TrueType or WOFF file of a `src` list such as
/// `url("a.woff2") format("woff2"), url(a.ttf) format("truetype")` (WOFF 2
/// is not read).
fn font_url(src: &str) -> Result<Option<String>, DocumentError> {
    for part in src.split("url(").skip(1) {
        let Some((url, rest)) = part.split_once(')') else {
            continue;
        };
        let url = url.trim().trim_matches(|c| c == '"' || c == '\'').trim();
        let format = rest.split(',').next().unwrap_or_default();
        let has = |word: &str| {
            format
                .as_bytes()
                .windows(word.len())
                .any(|window| window.eq_ignore_ascii_case(word.as_bytes()))
        };
        let by_name = [".ttf", ".otf", ".ttc", ".woff"].iter().any(|e| {
            url.len() >= e.len()
                && url.as_bytes()[url.len() - e.len()..].eq_ignore_ascii_case(e.as_bytes())
        });
        let by_format = has("truetype") || has("opentype") || (has("woff") && !has("woff2"));
        let other = has("woff2") || has("svg") || has("embedded");
        if (by_name && !other) || by_format {
            return fallible::string(url).map(Some);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(css: &str) -> Rules {
        let mut rules = Rules::default();
        let mut parser = Parser::new("OEBPS/css/").unwrap();
        // Fed in small pieces, as from a stream.
        for piece in css.as_bytes().chunks(7) {
            parser.feed(core::str::from_utf8(piece).unwrap(), &mut rules);
        }
        rules
    }

    #[test]
    fn strings_comments_and_space_do_not_end_rules() {
        let r = rules(
            ".a { content: \"} ;\"; text-align: center } /*/ .b { text-align: right } */ \
             .c { font-family: 'x}y'; font-weight: bold }",
        );
        assert_eq!(r.props("p", "a").align, Some(Align::Center));
        assert_eq!(r.props("p", "b").align, None);
        assert_eq!(r.props("p", "c").bold, Some(true));
        // A rule after much white space is read whole.
        let mut css = String::from(".d { text-align: left }");
        css.push_str(&" ".repeat(10_000));
        css.push_str(".e { text-align: right }");
        let r = rules(&css);
        assert_eq!(r.props("p", "e").align, Some(Align::Right));
    }

    #[test]
    fn embedded_fonts_are_bounded() {
        let mut css = String::new();
        for i in 0..1000 {
            css.push_str(&alloc::format!(
                "@font-face {{ font-family: F; src: url(f{i}.ttf) }}"
            ));
        }
        assert_eq!(rules(&css).faces.len(), MAX_FACES);
    }

    #[test]
    fn reads_simple_rules() {
        let r = rules(
            "@charset \"utf-8\"; /* title page */ h1, h2 { text-align: center } \
             .center{text-align:center;} p.bold { font-weight: bold } \
             @media print { p { display: none } } div p { font-style: italic } \
             .hide { display: none } em.normal { font-style: normal !important }",
        );
        assert_eq!(r.rules.len(), 6);
        assert_eq!(r.props("h2", "").align, Some(Align::Center));
        assert_eq!(r.props("p", "x center").align, Some(Align::Center));
        assert_eq!(r.props("p", "bold").bold, Some(true));
        assert_eq!(r.props("div", "bold").bold, None);
        assert_eq!(r.props("span", "hide").hidden, Some(true));
        assert_eq!(r.props("em", "normal").italic, Some(false));
        // Descendant selectors and @media blocks are skipped.
        assert_eq!(r.props("p", "").italic, None);
        assert_eq!(r.props("p", "").hidden, None);
    }

    #[test]
    fn remembered_props_are_the_props() {
        let mut r = rules("p { text-align: right } .a { font-weight: bold } p.b { display: none }");
        let long = "x ".repeat(40);
        let elements = [
            ("p", ""),
            ("p", "a"),
            ("pa", ""),
            ("p", "b"),
            ("p", "a b"),
            ("span", "a"),
            ("p", long.as_str()),
            ("div", ""),
            ("h1", "a"),
            ("em", ""),
            ("i", ""),
        ];
        for _ in 0..3 {
            for (tag, classes) in elements {
                assert_eq!(r.props_remembered(tag, classes), r.props(tag, classes));
            }
        }
        // Rules added later, the book's and one's own, apply at once.
        Parser::new("")
            .unwrap()
            .feed("p { text-align: center }", &mut r);
        assert_eq!(r.props_remembered("p", "").align, Some(Align::Center));
        r.add_own(".a { font-weight: normal }");
        assert_eq!(r.props_remembered("p", "a").bold, Some(false));
        for (tag, classes) in elements {
            assert_eq!(r.props_remembered(tag, classes), r.props(tag, classes));
        }
    }

    #[test]
    fn reads_indents_and_spaces() {
        assert_eq!(length("1.2em"), Some(120));
        assert_eq!(length("0"), Some(0));
        assert_eq!(length("12pt"), Some(100));
        assert_eq!(length("8px"), Some(50));
        assert_eq!(length("5%"), None);
        assert_eq!(length("auto"), None);
        let r = rules(
            ".noindent { text-indent:0em; margin-top:1.2em }              .head { padding-top: 6em; padding-bottom: 1.0em }              .hang { text-indent: -1.5em; margin: 0.5em 0 1em 2em } p { margin: 0 }",
        );
        let p = r.props("p", "noindent");
        assert_eq!(
            (p.indent, p.margin_top, p.margin_bottom),
            (Some(0), Some(120), Some(0))
        );
        let h = r.props("p", "head");
        assert_eq!((h.padding_top, h.padding_bottom), (Some(600), Some(100)));
        // A hanging indent, into the margin at the left.
        let hang = r.props("div", "hang");
        assert_eq!(hang.indent, Some(-150));
        assert_eq!((hang.margin_top, hang.margin_bottom), (Some(50), Some(100)));
        assert_eq!((hang.margin_left, hang.margin_right), (Some(200), Some(0)));
        assert_eq!(hang.sides(), (200, 0));
        let copy = Rules::decode(&r.encode());
        assert_eq!(copy.rules, r.rules);
    }

    #[test]
    fn reads_sides_line_heights_and_marks() {
        let r = rules(
            ".poem { margin-left: 3em; padding-right: 10%; line-height: 1.6 }              .tight { line-height: 120% } .px { line-height: 24px } .normal { line-height: normal }              .two { margin: 1em 2em } .neg { margin-left: -2em }              .up { vertical-align: super } .down { vertical-align: sub }              .u { text-decoration: underline } .gone { text-decoration: line-through underline }              .plain { text-decoration: none }",
        );
        let p = |class| r.props("p", class);
        assert_eq!(p("poem").sides(), (300, 250));
        assert_eq!(p("poem").line_height, Some(160));
        assert_eq!(p("tight").line_height, Some(120));
        assert_eq!(p("px").line_height, Some(150));
        assert_eq!(p("normal").line_height, None);
        // Two values: top and bottom, then the sides.
        assert_eq!(p("two").sides(), (200, 200));
        assert_eq!(p("neg").margin_left, Some(0));
        assert_eq!(p("up").vertical, Some(Vertical::Super));
        assert_eq!(p("down").vertical, Some(Vertical::Sub));
        assert_eq!((p("u").underline, p("u").strike), (Some(true), None));
        assert_eq!(
            (p("gone").underline, p("gone").strike),
            (Some(true), Some(true))
        );
        assert_eq!(
            (p("plain").underline, p("plain").strike),
            (Some(false), Some(false))
        );
        let copy = Rules::decode(&r.encode());
        assert_eq!(copy.rules, r.rules);
    }

    #[test]
    fn list_styles_are_read_and_survive_the_cache() {
        let r = rules(
            ".a { list-style-type: Lower-Roman } .b { list-style: url(x.png) square inside } \
             .c { list-style-type: hiragana } .d { list-style: none }",
        );
        let p = |class| r.props("li", class);
        assert_eq!(p("a").list_style, Some(ListStyle::LowerRoman));
        assert_eq!(p("b").list_style, Some(ListStyle::Square));
        assert_eq!(p("c").list_style, None);
        assert_eq!(p("d").list_style, Some(ListStyle::None));
        let copy = Rules::decode(&r.encode());
        assert_eq!(copy.rules, r.rules);
        assert_eq!(Props::decode(" y99").list_style, None);
    }

    #[test]
    fn drop_capital_styles_survive_the_cache() {
        let r = rules(
            ".dc { float: left; font-size: 3em } .il { initial-letter: 3 2 } .no { float: none }",
        );
        let p = |class| r.props("span", class);
        assert_eq!(
            (p("dc").float_left, p("dc").initial_letter),
            (Some(true), None)
        );
        assert_eq!(p("il").initial_letter, Some(3));
        assert_eq!(p("no").float_left, Some(false));
        let copy = Rules::decode(&r.encode());
        assert_eq!(copy.rules, r.rules);
    }

    #[test]
    fn reads_font_sizes() {
        let r = rules(
            ".big { font-size: 1.3em } .half { font-size: 50% } .pt { font-size: 9pt } \
             .k { font-size: x-large } .s { font-size: smaller } .bad { font-size: inherit }",
        );
        let size = |class| r.props("p", class).size;
        assert_eq!(size("big"), Some(FontSize::Relative(130)));
        assert_eq!(size("half"), Some(FontSize::Relative(50)));
        assert_eq!(size("pt"), Some(FontSize::Absolute(75)));
        assert_eq!(size("k"), Some(FontSize::Absolute(150)));
        assert_eq!(size("s"), Some(FontSize::Relative(83)));
        assert_eq!(size("bad"), None);
        // Relative sizes compound, and all stay readable.
        assert_eq!(FontSize::Relative(130).of(120), 156);
        assert_eq!(FontSize::Absolute(75).of(200), 75);
        assert_eq!(FontSize::Relative(50).of(50), 50);
        let copy = Rules::decode(&r.encode());
        assert_eq!(copy.rules, r.rules);
    }

    #[test]
    fn sides_are_absolute_and_start_and_end_logical() {
        for (value, align) in [
            ("left", Align::Left),
            ("right", Align::Right),
            ("LEFT ", Align::Left),
            ("start", Align::Start),
            ("end", Align::End),
            ("center", Align::Center),
            ("justify", Align::Justify),
        ] {
            let r = rules(&alloc::format!("p {{ text-align: {value} }}"));
            assert_eq!(r.props("p", "").align, Some(align), "{value}");
            let copy = Rules::decode(&r.encode());
            assert_eq!(copy.props("p", "").align, Some(align), "{value} cached");
        }
    }

    #[test]
    fn classes_outrank_elements_and_survive_the_cache() {
        let r = rules(".left { text-align: left } p { text-align: center }");
        assert_eq!(r.props("p", "left").align, Some(Align::Left));
        assert_eq!(r.props("p", "").align, Some(Align::Center));
        let copy = Rules::decode(&r.encode());
        assert_eq!(copy.rules, r.rules);
    }

    #[test]
    fn reads_embedded_fonts_and_families() {
        let r = rules(
            "@font-face { font-family: 'My Serif'; src: url(../fonts/a.woff2) format('woff2'), \
             url(\"../fonts/MySerif.ttf\") format('truetype') } \
             @font-face { font-family: \"My Serif\"; font-weight: bold; font-style: italic; \
             src: url(../fonts/MySerif-BI.otf) } \
             @font-face { font-family: Web; src: url(web.woff) } \
             @font-face { font-family: Newer; src: url(n.woff2) format('woff2') } \
             p { font-family: \"My Serif\", serif } h1 { font-family: sans-serif } \
             .other { font-family: Unknown, serif }",
        );
        // "Newer" only has a WOFF 2 file, so it is not embedded.
        assert_eq!(r.families, ["my serif", "web", "unknown"]);
        assert_eq!(
            r.faces,
            [
                FontFace {
                    family: 1,
                    bold: false,
                    italic: false,
                    path: String::from("OEBPS/fonts/MySerif.ttf"),
                },
                FontFace {
                    family: 1,
                    bold: true,
                    italic: true,
                    path: String::from("OEBPS/fonts/MySerif-BI.otf"),
                },
                FontFace {
                    family: 2,
                    bold: false,
                    italic: false,
                    path: String::from("OEBPS/css/web.woff"),
                },
            ]
        );
        assert_eq!(r.props("p", "").family, Some(1));
        assert_eq!(r.props("h1", "").family, Some(0));
        assert_eq!(r.props("p", "other").family, Some(3));
        let copy = Rules::decode(&r.encode());
        assert_eq!(copy.families, r.families);
        assert_eq!(copy.props("p", "").family, Some(1));
    }
}

#[cfg(test)]
mod hostile_tests;
