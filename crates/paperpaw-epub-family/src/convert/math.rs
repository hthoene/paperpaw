//! MathML as text that reads: the formula of a `<math>` element in its
//! linear form (`a/b` for a fraction, `x²` or `x^(n+1)`, `√(x)`, `a_i`),
//! built while the element is read and then given to the chapter as text, an
//! inline one in a paragraph or, with `display="block"`, a block of its own.
//!
//! Only presentation markup is read; what `<semantics>` carries besides its
//! first child (`<annotation>`) is left out.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::xml::{Attrs, try_decode};

use super::{Done, Formula};
use crate::fallible;

/// Most text of a formula kept (more is left out).
const MAX_TEXT: usize = 4096;
/// Deepest nesting kept (deeper elements are read as part of their parent).
const MAX_DEPTH: usize = 32;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    /// `mi`, `mn`, `mtext`, `ms`: text.
    Text,
    /// `mo`: an operator or a fence.
    Operator,
    /// `mrow` and every element that is read as its children in a row.
    Row,
    Frac,
    Sup,
    Sub,
    SubSup,
    Under,
    Over,
    UnderOver,
    Sqrt,
    Root,
    Fenced,
    Table,
    TableRow,
    /// `semantics`, `maction`: the first child.
    First,
    /// `annotation`, `mphantom`, `none`, `mprescripts`: nothing.
    Nothing,
    Space,
}

impl Kind {
    fn of(tag: &str) -> Self {
        match tag {
            "mi" | "mn" | "mtext" | "ms" => Self::Text,
            "mo" => Self::Operator,
            "mfrac" => Self::Frac,
            "msup" => Self::Sup,
            "msub" => Self::Sub,
            "msubsup" => Self::SubSup,
            "munder" => Self::Under,
            "mover" => Self::Over,
            "munderover" => Self::UnderOver,
            "msqrt" => Self::Sqrt,
            "mroot" => Self::Root,
            "mfenced" => Self::Fenced,
            "mtable" => Self::Table,
            "mtr" | "mlabeledtr" => Self::TableRow,
            "semantics" | "maction" => Self::First,
            "annotation" | "annotation-xml" | "mphantom" | "none" | "mprescripts" => Self::Nothing,
            "mspace" => Self::Space,
            _ => Self::Row,
        }
    }
}

struct Node {
    kind: Kind,
    /// The text of a leaf.
    text: String,
    /// What the children came to.
    kids: Vec<String>,
    /// `mfenced`'s opening and closing fence and its separator.
    fence: Option<(String, String, char)>,
    /// `ms` is a string in quotes.
    quoted: bool,
}

/// A formula being read.
struct Math {
    stack: Vec<Node>,
    /// Elements deeper than [`MAX_DEPTH`] that are open.
    deep: u32,
    bytes: usize,
    /// A block of its own (`display="block"`).
    display: bool,
    /// The author's own text for it (`alttext`).
    alt: Option<String>,
}

impl Math {
    /// The formula of the `<math>` element with `attrs`.
    fn new(attrs: &Attrs<'_>) -> Result<Self, DocumentError> {
        let get = |name: &str| attrs.try_get(name).map_err(|_| fallible::OOM);
        let alt = match get("alttext")? {
            Some(alt) => Some(fallible::string(alt.trim())?),
            None => None,
        };
        let mut math = Self {
            stack: Vec::new(),
            deep: 0,
            bytes: 0,
            display: get("display")?.as_deref() == Some("block"),
            alt,
        };
        math.push("math", attrs)?;
        Ok(math)
    }

    fn push(&mut self, tag: &str, attrs: &Attrs<'_>) -> Result<(), DocumentError> {
        let kind = Kind::of(tag);
        let fence = if kind == Kind::Fenced {
            let get = |name: &str, default: &str| -> Result<String, DocumentError> {
                match attrs.try_get(name).map_err(|_| fallible::OOM)? {
                    Some(value) => fallible::string(value.trim()),
                    None => fallible::string(default),
                }
            };
            let separator = get("separators", ",")?.chars().next().unwrap_or(',');
            Some((get("open", "(")?, get("close", ")")?, separator))
        } else {
            None
        };
        fallible::push(
            &mut self.stack,
            Node {
                kind,
                text: String::new(),
                kids: Vec::new(),
                fence,
                quoted: tag == "ms",
            },
        )
    }
}

impl Math {
    /// Ends the innermost open element, as what it renders to in its parent.
    fn close_innermost(&mut self) -> Result<(), DocumentError> {
        if let Some(node) = self.stack.pop() {
            let rendered = render(node)?;
            if let Some(parent) = self.stack.last_mut() {
                fallible::push(&mut parent.kids, rendered)?;
            }
        }
        Ok(())
    }
}

impl Formula for Math {
    fn start(&mut self, tag: &str, attrs: &Attrs<'_>) -> Result<(), DocumentError> {
        if self.stack.len() >= MAX_DEPTH {
            self.deep += 1;
            return Ok(());
        }
        self.push(tag, attrs)
    }

    fn text(&mut self, text: &str, cdata: bool) -> Result<(), DocumentError> {
        let Some(node) = self.stack.last_mut() else {
            return Ok(());
        };
        if !matches!(node.kind, Kind::Text | Kind::Operator) || self.bytes >= MAX_TEXT {
            return Ok(());
        }
        let text = if cdata {
            alloc::borrow::Cow::Borrowed(text)
        } else {
            try_decode(text).map_err(|_| fallible::OOM)?
        };
        self.bytes += text.len();
        fallible::push_str(&mut node.text, &text)
    }

    fn end(&mut self, tag: &str) -> Result<Option<Done>, DocumentError> {
        let math = tag.eq_ignore_ascii_case("math");
        if self.deep > 0 && !math {
            self.deep -= 1;
            return Ok(None);
        }
        self.deep = 0;
        // `</math>` ends the elements left open in it too.
        while math && self.stack.len() > 1 {
            self.close_innermost()?;
        }
        let Some(node) = self.stack.pop() else {
            return Ok(None);
        };
        let rendered = render(node)?;
        match self.stack.last_mut() {
            Some(parent) => {
                fallible::push(&mut parent.kids, rendered)?;
                Ok(None)
            }
            None => {
                let mut text = tidy(&rendered)?;
                if text.is_empty()
                    && let Some(alt) = self.alt.take()
                {
                    text = alt;
                }
                Ok(Some(Done {
                    text,
                    display: self.display,
                    markup: None,
                }))
            }
        }
    }
}

/// Whether `s` is one token: a letter, a number, a name (no parentheses
/// needed around it).
fn simple(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '.')
}

/// Whether `s` is in one pair of parentheses.
fn enclosed(s: &str) -> bool {
    let Some(inner) = s.strip_prefix('(').and_then(|s| s.strip_suffix(')')) else {
        return false;
    };
    let mut depth = 0u32;
    inner.chars().all(|c| {
        match c {
            '(' => depth += 1,
            ')' => return depth.checked_sub(1).map(|d| depth = d).is_some(),
            _ => {}
        }
        true
    })
}

/// `s`, in parentheses unless it is [`simple`].
fn wrapped(s: String) -> Result<String, DocumentError> {
    if simple(&s) || enclosed(&s) {
        Ok(s)
    } else {
        fallible::try_format!("({s})")
    }
}

/// A script (`x²`, `x^n`, `x_(i+1)`): `mark` `^` or `_`.
fn script(mark: char, s: &str) -> Result<String, DocumentError> {
    if mark == '^'
        && let Some(sup) = match s {
            "1" => Some('¹'),
            "2" => Some('²'),
            "3" => Some('³'),
            _ => None,
        }
    {
        return fallible::try_format!("{sup}");
    }
    if simple(s) {
        fallible::try_format!("{mark}{s}")
    } else if s.is_empty() {
        Ok(String::new())
    } else {
        fallible::try_format!("{mark}({s})")
    }
}

/// Operators that large formulas hang their limits on: a space follows the
/// limits.
fn is_large(base: &str) -> bool {
    matches!(
        base,
        "∑" | "∏" | "∫" | "∮" | "∬" | "∭" | "⋃" | "⋂" | "lim" | "max" | "min" | "sup" | "inf"
    )
}

/// The combining mark an accent over a letter is.
fn accent(over: &str) -> Option<char> {
    Some(match over {
        "¯" | "‾" | "−" | "_" | "ˉ" => '\u{304}',
        "^" | "ˆ" => '\u{302}',
        "~" | "˜" | "∼" => '\u{303}',
        "→" | "⃗" => '\u{20d7}',
        "." | "˙" => '\u{307}',
        "¨" => '\u{308}',
        _ => return None,
    })
}

/// How an operator reads: spaces around those between two things, none
/// around fences, one after a comma.
fn operator(op: &str) -> Result<String, DocumentError> {
    let op = op.trim();
    // The invisible operators (function application, times, separator, plus).
    if op.chars().all(|c| ('\u{2061}'..='\u{2064}').contains(&c)) {
        return Ok(String::new());
    }
    let spaced = matches!(
        op,
        "+" | "-"
            | "−"
            | "×"
            | "÷"
            | "·"
            | "⋅"
            | "*"
            | "±"
            | "∓"
            | "="
            | "≠"
            | "≈"
            | "≡"
            | "<"
            | ">"
            | "≤"
            | "≥"
            | "⩽"
            | "⩾"
            | "≪"
            | "≫"
            | "→"
            | "←"
            | "↔"
            | "⇒"
            | "⇐"
            | "⇔"
            | "↦"
            | "∈"
            | "∉"
            | "⊂"
            | "⊆"
            | "⊃"
            | "⊇"
            | "∪"
            | "∩"
            | "∧"
            | "∨"
            | "∼"
            | "∝"
            | "∘"
            | "⊕"
            | "⊗"
            | "∖"
            | "∣"
            | ":="
    );
    if spaced {
        fallible::try_format!(" {op} ")
    } else if matches!(op, "," | ";") {
        fallible::try_format!("{op} ")
    } else {
        fallible::string(op)
    }
}

/// `text` with its runs of white space made one space.
fn collapse(text: &str) -> Result<String, DocumentError> {
    let mut out = fallible::string_with(text.len())?;
    for word in text.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    Ok(out)
}

/// The children of a row as one text: a name of a function (`sin`) is parted
/// from what follows it.
fn row(kids: &[String]) -> Result<String, DocumentError> {
    let mut out = String::new();
    let mut last_word = false;
    for kid in kids {
        let first = kid.chars().next();
        if last_word && first.is_some_and(char::is_alphanumeric) {
            fallible::push_char(&mut out, ' ')?;
        }
        fallible::push_str(&mut out, kid)?;
        last_word = kid.chars().count() >= 2 && kid.chars().all(char::is_alphabetic);
    }
    Ok(out)
}

/// Child `i`, taken (empty where there is none).
fn take(kids: &mut [String], i: usize) -> String {
    kids.get_mut(i).map(core::mem::take).unwrap_or_default()
}

/// `kids` one after the other, `separator` between them.
fn joined(kids: &[String], separator: &str) -> Result<String, DocumentError> {
    let mut out = String::new();
    for (i, kid) in kids.iter().enumerate() {
        if i > 0 {
            fallible::push_str(&mut out, separator)?;
        }
        fallible::push_str(&mut out, kid)?;
    }
    Ok(out)
}

fn render(node: Node) -> Result<String, DocumentError> {
    let mut kids = node.kids;
    Ok(match node.kind {
        Kind::Text => {
            let text = collapse(&node.text)?;
            if node.quoted {
                fallible::try_format!("\"{text}\"")?
            } else {
                text
            }
        }
        Kind::Operator => operator(&node.text)?,
        Kind::Space => fallible::string(" ")?,
        Kind::Nothing => String::new(),
        Kind::First => take(&mut kids, 0),
        Kind::Row => row(&kids)?,
        Kind::Frac => {
            let (num, den) = (take(&mut kids, 0), take(&mut kids, 1));
            fallible::try_format!("{}/{}", wrapped(num)?, wrapped(den)?)?
        }
        Kind::Sup | Kind::Sub | Kind::Over | Kind::Under => {
            let (base, part) = (take(&mut kids, 0), take(&mut kids, 1));
            let (mark, accents) = match node.kind {
                Kind::Sup => ('^', false),
                Kind::Sub | Kind::Under => ('_', false),
                _ => ('^', true),
            };
            if accents && let Some(combining) = accent(&part) {
                fallible::try_format!("{base}{combining}")?
            } else {
                let large = is_large(&base);
                let mut out = base;
                fallible::push_str(&mut out, &script(mark, &part)?)?;
                if large {
                    fallible::push_char(&mut out, ' ')?;
                }
                out
            }
        }
        Kind::SubSup | Kind::UnderOver => {
            let base = take(&mut kids, 0);
            let (low, high) = (take(&mut kids, 1), take(&mut kids, 2));
            let large = is_large(&base);
            let mut out = base;
            fallible::push_str(&mut out, &script('_', &low)?)?;
            fallible::push_str(&mut out, &script('^', &high)?)?;
            if large {
                fallible::push_char(&mut out, ' ')?;
            }
            out
        }
        Kind::Sqrt => fallible::try_format!("√{}", wrapped(row(&kids)?)?)?,
        Kind::Root => {
            let (radicand, index) = (take(&mut kids, 0), take(&mut kids, 1));
            let radical = wrapped(radicand)?;
            match index.as_str() {
                "2" | "" => fallible::try_format!("√{radical}")?,
                "3" => fallible::try_format!("³√{radical}")?,
                _ => fallible::try_format!("{}√{radical}", wrapped(index)?)?,
            }
        }
        Kind::Fenced => {
            let (open, close, separator) = node
                .fence
                .unwrap_or_else(|| (String::from("("), String::from(")"), ','));
            let inside = joined(&kids, &fallible::try_format!("{separator} ")?)?;
            fallible::try_format!("{open}{inside}{close}")?
        }
        Kind::TableRow => joined(&kids, "   ")?,
        Kind::Table => joined(&kids, "\n")?,
    })
}

/// The finished text: spaces collapsed, none inside brackets, none between a
/// sign and what it signs.
fn tidy(text: &str) -> Result<String, DocumentError> {
    let mut out = fallible::string_with(text.len())?;
    for (n, line) in text.split('\n').enumerate() {
        if n > 0 {
            out.push('\n');
        }
        let mut prev: Option<&str> = None;
        let mut glue = false;
        for word in line.split(' ').filter(|w| !w.is_empty()) {
            let space = prev.is_some_and(|p| {
                !(glue
                    || p.ends_with(['(', '[', '{'])
                    || word.starts_with([')', ']', '}', ',', ';']))
            });
            if space {
                out.push(' ');
            }
            out.push_str(word);
            let sign = matches!(word, "−" | "-" | "+" | "±" | "∓");
            // A sign that starts a row, or follows an operator or an opening
            // bracket, belongs to what comes next.
            glue = sign
                && prev.is_none_or(|p| {
                    p.ends_with(['(', '[', '{'])
                        || p.chars()
                            .all(|c| !c.is_alphanumeric() && !matches!(c, ')' | ']' | '}'))
                });
            prev = Some(word);
        }
    }
    Ok(out)
}

/// The text of a foreign element's markup (`convert/foreign.rs` keeps it as
/// `<math …>…</math>` in the chapter's cache): the formula in its linear
/// form, `\n` between the lines of a matrix. `None` for markup that is no
/// `<math>`, gives no text or finds no room.
pub fn text_of(markup: &str) -> Option<String> {
    use paperpaw_sdk::xml::{Token, Tokenizer, local_name};
    let mut math: Option<Math> = None;
    let mut done: Option<String> = None;
    let mut failed = false;
    let mut feed = |token: Token<'_>| {
        if failed || done.is_some() {
            return;
        }
        let step = (|| -> Result<Option<Done>, DocumentError> {
            match token {
                Token::Start {
                    name,
                    attrs,
                    self_closing,
                } => {
                    match math.as_mut() {
                        Some(math) => math.start(local_name(name), &attrs)?,
                        None if local_name(name) == "math" => math = Some(Math::new(&attrs)?),
                        None => return Ok(None),
                    }
                    match (self_closing, math.as_mut()) {
                        (true, Some(math)) => math.end(local_name(name)),
                        _ => Ok(None),
                    }
                }
                Token::End { name } => math
                    .as_mut()
                    .map_or(Ok(None), |math| math.end(local_name(name))),
                Token::Text(text) => {
                    if let Some(math) = math.as_mut() {
                        math.text(&text, false)?;
                    }
                    Ok(None)
                }
                Token::CData(text) => {
                    if let Some(math) = math.as_mut() {
                        math.text(text, true)?;
                    }
                    Ok(None)
                }
            }
        })();
        match step {
            Ok(Some(finished)) => done = Some(finished.text),
            Ok(None) => {}
            Err(_) => failed = true,
        }
    };
    Tokenizer::parse(markup.as_bytes(), &mut feed);
    done.filter(|text| !text.is_empty())
}
