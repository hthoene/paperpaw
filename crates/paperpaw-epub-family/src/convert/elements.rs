//! What elements are and set: their styles by name, stylesheet and
//! attributes, which are blocks, headings, void or a page's furniture.

use paperpaw_sdk::document::{Align, DocumentError, TextStyle};

use crate::css::{self, Props, Rules};

/// Words in class names and ids of page furniture (`site-nav`,
/// `vector-dropdown`, `mw-editsection`, …).
const FURNITURE: [&str; 24] = [
    "nav",
    "navbar",
    "navigation",
    "navbox",
    "menu",
    "dropdown",
    "sidebar",
    "banner",
    "cookie",
    "cookies",
    "share",
    "sharing",
    "social",
    "related",
    "comments",
    "advert",
    "ads",
    "promo",
    "popup",
    "modal",
    "noprint",
    "breadcrumb",
    "breadcrumbs",
    "editsection",
];

/// What an element sets: defaults for its name, then the stylesheet
/// rules, then its own `align` and `style` attributes (CSS only with
/// `styles`).
pub(super) fn element_props(
    tag: &str,
    attrs: &paperpaw_sdk::xml::Attrs<'_>,
    rules: &mut Rules,
    styles: bool,
) -> Result<Props, DocumentError> {
    let get = |name: &str| attrs.try_get(name).map_err(|_| crate::fallible::OOM);
    let mut props = Props::default();
    match tag {
        "b" | "strong" | "th" => props.bold = Some(true),
        "i" | "em" | "cite" | "dfn" | "var" => props.italic = Some(true),
        "center" => props.align = Some(Align::Center),
        "sup" => props.vertical = Some(css::Vertical::Super),
        "sub" => props.vertical = Some(css::Vertical::Sub),
        "u" | "ins" => props.underline = Some(true),
        "s" | "strike" | "del" => props.strike = Some(true),
        // Set in as browsers set them in, if a little less.
        "blockquote" => (props.margin_left, props.margin_right) = (Some(150), Some(150)),
        "ul" | "ol" | "dd" => props.margin_left = Some(150),
        _ => {}
    }
    // The marker an old `type` attribute asks for, below the stylesheet's.
    if matches!(tag, "ol" | "ul" | "li")
        && let Some(value) = get("type")?
    {
        props.list_style = super::lists::type_attribute(&value);
    }
    let classes = get("class")?.unwrap_or_default();
    props.apply(&rules.props_remembered(tag, &classes));
    if let Some(value) = get("align")? {
        props.set_align(&value);
    }
    if styles && let Some(css) = get("style")? {
        props.apply(&rules.parse_style(&css));
    }
    Ok(props)
}

/// `style` with bold, italic, small capitals, raised or lowered, the
/// lines under or through and the font family as `props` sets them.
pub(super) fn styled(style: TextStyle, props: &Props) -> TextStyle {
    let mut bits = style.bits();
    let (sup, sub) = match props.vertical {
        Some(css::Vertical::Super) => (Some(true), Some(false)),
        Some(css::Vertical::Sub) => (Some(false), Some(true)),
        Some(css::Vertical::Baseline) => (Some(false), Some(false)),
        None => (None, None),
    };
    for (flag, on) in [
        (TextStyle::BOLD, props.bold),
        (TextStyle::ITALIC, props.italic),
        (TextStyle::SMALL_CAPS, props.small_caps),
        (TextStyle::SUPERSCRIPT, sup),
        (TextStyle::SUBSCRIPT, sub),
        (TextStyle::UNDERLINE, props.underline),
        (TextStyle::STRIKE, props.strike),
    ] {
        match on {
            Some(true) => bits |= flag.bits(),
            Some(false) => bits &= !flag.bits(),
            None => {}
        }
    }
    let style = TextStyle::from_bits(bits);
    match props.family {
        Some(family) => style.with_family(family),
        None => style,
    }
}

/// Elements without content or end tag in HTML.
pub(super) fn is_void(tag: &str) -> bool {
    const VOID: [&str; 13] = [
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source",
        "track", "wbr",
    ];
    VOID.iter().any(|void| void.eq_ignore_ascii_case(tag))
}

/// Parts of a web page that are not reading matter: navigation, forms,
/// menus and the like, by element, ARIA role or class name. Headers and
/// footers belong to the reading matter when it is marked (an article's
/// title), else they are the site's.
pub(super) fn furniture(
    tag: &str,
    attrs: &paperpaw_sdk::xml::Attrs<'_>,
    focus: bool,
) -> Result<bool, DocumentError> {
    let get = |name: &str| attrs.try_get(name).map_err(|_| crate::fallible::OOM);
    let role = get("role")?;
    if matches!(
        role.as_deref(),
        Some("navigation" | "search" | "banner" | "contentinfo" | "complementary" | "menu")
    ) || get("aria-hidden")?.as_deref() == Some("true")
    {
        return Ok(true);
    }
    // A class or id naming furniture starts or ends with its word
    // (`nav-links`, `site-nav`); words in between describe a state
    // (`main-menu-pinned-disabled` on `<body>`).
    let furniture_word = |word: &str| FURNITURE.iter().any(|f| f.eq_ignore_ascii_case(word));
    let named = |name: &str| -> Result<bool, DocumentError> {
        Ok(get(name)?.is_some_and(|value| {
            value.split_whitespace().any(|token| {
                let mut words = token.split(['-', '_']).filter(|w| !w.is_empty());
                let first = words.next();
                let last = words.next_back();
                first.is_some_and(furniture_word) || last.is_some_and(furniture_word)
            })
        }))
    };
    let frame = matches!(tag, "html" | "body" | "main" | "article");
    if !frame && (named("class")? || named("id")?) {
        return Ok(true);
    }
    Ok(matches!(
        tag,
        "nav"
            | "aside"
            | "form"
            | "noscript"
            | "iframe"
            | "button"
            | "select"
            | "textarea"
            | "template"
            | "dialog"
            | "menu"
    ) || (!focus && matches!(tag, "header" | "footer")))
}

pub(super) fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "blockquote"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "aside"
            | "nav"
            | "figure"
            | "figcaption"
            | "dt"
            | "dd"
            | "dl"
            | "ul"
            | "ol"
            | "tr"
            | "table"
            | "address"
            | "center"
            | "body"
            | "caption"
    )
}

pub(super) fn heading_level(tag: &str) -> Option<u8> {
    match tag.as_bytes() {
        [b'h', d @ b'1'..=b'6'] => Some(d - b'0'),
        _ => None,
    }
}

/// The lines of a drop capital an inline element starting a paragraph
/// asks for, set in `size` (percent of body text) within text of `parent`
/// size: CSS `initial-letter`, or a letter floated left half as large
/// again as the text around it or more, a line for each 1.2 times (as
/// the layout keeps it, two to five); 0 for none.
pub(super) fn drop_cap(props: &Props, size: Option<u16>, parent: Option<u16>) -> u8 {
    match props.initial_letter {
        Some(lines) => lines,
        None if props.float_left == Some(true) => {
            let ratio = f32::from(size.unwrap_or(100)) / f32::from(parent.unwrap_or(100).max(1));
            match ratio >= 1.5 {
                true => (ratio / 1.2 + 0.5).clamp(2.0, 5.0) as u8,
                false => 0,
            }
        }
        None => 0,
    }
}

/// Whether a paragraph's `text` so far, once the element that asked for
/// a drop capital ends, is one: a letter, and an opening quote or two.
pub(super) fn is_capital(text: &str) -> bool {
    text.chars().filter(|c| c.is_alphanumeric()).count() == 1 && text.chars().count() <= 3
}
