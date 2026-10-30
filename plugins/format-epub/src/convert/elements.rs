//! What elements are and set: their styles by name, stylesheet and
//! attributes, which are blocks, headings, void or a page's furniture.

use paperpaw_sdk::document::{Align, TextStyle};

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
) -> Props {
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
    let classes = attrs.get("class").unwrap_or_default();
    props.apply(&rules.props_remembered(tag, &classes));
    if let Some(value) = attrs.get("align") {
        props.set_align(&value);
    }
    if styles && let Some(css) = attrs.get("style") {
        props.apply(&rules.parse_style(&css));
    }
    props
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
    matches!(
        tag.to_ascii_lowercase().as_str(),
        "area"
            | "base"
            | "br"
            | "col"
            | "embed"
            | "hr"
            | "img"
            | "input"
            | "link"
            | "meta"
            | "source"
            | "track"
            | "wbr"
    )
}

/// Parts of a web page that are not reading matter: navigation, forms,
/// menus and the like, by element, ARIA role or class name. Headers and
/// footers belong to the reading matter when it is marked (an article's
/// title), else they are the site's.
pub(super) fn furniture(tag: &str, attrs: &paperpaw_sdk::xml::Attrs<'_>, focus: bool) -> bool {
    let role = attrs.get("role");
    if matches!(
        role.as_deref(),
        Some("navigation" | "search" | "banner" | "contentinfo" | "complementary" | "menu")
    ) || attrs.get("aria-hidden").as_deref() == Some("true")
    {
        return true;
    }
    // A class or id naming furniture starts or ends with its word
    // (`nav-links`, `site-nav`); words in between describe a state
    // (`main-menu-pinned-disabled` on `<body>`).
    let furniture_word = |word: &str| FURNITURE.contains(&word.to_ascii_lowercase().as_str());
    let named = |name: &str| {
        attrs.get(name).is_some_and(|value| {
            value.split_whitespace().any(|token| {
                let mut words = token.split(['-', '_']).filter(|w| !w.is_empty());
                let first = words.next();
                let last = words.next_back();
                first.is_some_and(furniture_word) || last.is_some_and(furniture_word)
            })
        })
    };
    let frame = matches!(tag, "html" | "body" | "main" | "article");
    if !frame && (named("class") || named("id")) {
        return true;
    }
    matches!(
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
    ) || (!focus && matches!(tag, "header" | "footer"))
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
