//! Icons by name, e.g. `books` or `settings`, drawn from the icon font
//! bound to [`fonts::ICONS`](crate::builtin::fonts::ICONS).
//!
//! The names are those of [Tabler Icons](https://tabler.io/icons), all of
//! them known here by their hashes (the firmware shows no name, so it
//! does not carry them: [`names`] is for tools and tests);
//! the icon font holds the drawings of those PaperPaw uses (see
//! `tools/icons/subset.py`), icon packs add others (font packs that
//! extend it, see `tools/icons/pack.py`). An icon theme is a font with
//! other drawings at the same code points, bound to the role in place of
//! the default.
//!
//! Plugins give their menu entries icons by name
//! ([`Registrar::add_icon`](crate::Registrar::add_icon)); an icon without
//! a drawing draws nothing.
//!
//! A native plugin file (ADR 0024) built with `--cfg paperpaw_plugin`
//! (as `samples/hello-native` builds) asks the firmware for an icon's
//! character instead of carrying the names of all of them.

include!("icon_table.rs");

/// What an icon says rather than which it is: drawn with the icon a
/// plugin names for it ([`Registrar::add_icon`](crate::Registrar::add_icon),
/// e.g. `r.add_icon(icons::BACK, "arrow-big-left".into())`), else with
/// its default ([`default_of`]), else as PaperPaw draws it itself (the
/// arrows of headers and the keyboard's keys).
pub const BACK: &str = "core.icon.back";
pub const PREV: &str = "core.icon.prev";
pub const NEXT: &str = "core.icon.next";

/// The semantic arrow icon pointing the other way, for a UI that runs
/// right to left: back and previous point right, next points left.
pub(crate) fn turned(icon: &str) -> &str {
    match icon {
        BACK | PREV => NEXT,
        NEXT => PREV,
        other => other,
    }
}

/// Beside a button's hint where holding it does something too.
pub const HOLD: &str = "core.icon.hold";
/// In a button's hint while its press is being worked on.
pub const BUSY: &str = "core.icon.busy";
/// PaperPaw's paw: the busy mark in a corner of the screen.
pub const PAW: &str = "paw";
/// The keyboard's keys.
pub const SHIFT: &str = "core.icon.shift";
pub const DELETE: &str = "core.icon.delete";
pub const SPACE: &str = "core.icon.space";

/// The icon a semantic icon ([`BACK`] and the others) is drawn with
/// unless a plugin names another; `None` where PaperPaw draws it itself.
pub fn default_of(name: &str) -> Option<&'static str> {
    match name {
        HOLD => Some("hand-click"),
        BUSY => Some("paw"),
        _ => None,
    }
}

/// A name's hash, as `tools/icons/subset.py` counts it for [`HASHES`]
/// (FNV-1a, 32 bits).
#[cfg(not(all(paperpaw_plugin, target_os = "none")))]
fn hash(name: &str) -> u32 {
    name.bytes().fold(0x811c_9dc5, |hash: u32, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
    })
}

/// The character of icon `name` in the icon font; a name `U+F8000` is
/// that code point, for icons of one's own outside the Tabler names.
#[cfg(not(all(paperpaw_plugin, target_os = "none")))]
pub fn char(name: &str) -> Option<char> {
    code_point(name).or_else(|| by_name(name))
}

/// The character a name `U+` and hexadecimal digits names.
fn code_point(name: &str) -> Option<char> {
    let hex = name.strip_prefix("U+")?;
    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
}

/// The character of icon `name` in the icon font, as the firmware that
/// loaded this plugin file knows it.
#[cfg(all(paperpaw_plugin, target_os = "none"))]
pub fn char(name: &str) -> Option<char> {
    if let Some(ch) = code_point(name) {
        return Some(ch);
    }
    unsafe extern "C" {
        /// The firmware's table: `paperpaw-plugin.x` makes the word, the
        /// loader writes its address before the plugin's code runs.
        static PAPERPAW_API: *const paperpaw_native::Api;
    }
    // SAFETY: a table that lives as long as the firmware runs, written
    // before any code of the plugin ran.
    let api = unsafe { &*PAPERPAW_API };
    let has_icon = core::mem::offset_of!(paperpaw_native::Api, icon)
        + core::mem::size_of::<usize>()
        <= api.size as usize;
    if !has_icon {
        return None;
    }
    // SAFETY: the firmware reads `name` while it answers.
    let code = unsafe { (api.icon)(name.as_ptr(), name.len()) };
    core::char::from_u32(code).filter(|&c| c != '\0')
}

/// The character of the icon whose name's hash is that of `name`: the
/// names are not kept (#258), so one that is no icon's is taken for an
/// icon where the 32 bits happen to be the same (about one in a million).
#[cfg(not(all(paperpaw_plugin, target_os = "none")))]
fn by_name(name: &str) -> Option<char> {
    let at = HASHES.binary_search(&hash(name)).ok()?;
    char::from_u32(CODE_BASE + u32::from(*CODES.get(at)?))
}

/// What goes before an icon's character in the text `as_text` makes: a
/// private-use character no words begin with.
const AS_TEXT: char = '\u{e000}';

/// An icon's character as the text of a [`Hint::Text`](crate::Hint::Text)
/// the row of hints shows as that icon: for an icon whose name does not
/// live as long as [`Hint::Icon`](crate::Hint::Icon) asks, one a native
/// plugin named. `None` where there is no memory for it.
#[cfg(feature = "kernel")]
#[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
pub fn as_text(icon: char) -> Option<alloc::string::String> {
    let mut text = alloc::string::String::new();
    text.try_reserve_exact(AS_TEXT.len_utf8() + icon.len_utf8())
        .ok()?;
    text.push(AS_TEXT);
    text.push(icon);
    Some(text)
}

/// The icon a text of `as_text` is; `None` for any other text.
pub(crate) fn of_text(text: &str) -> Option<char> {
    let mut rest = text.strip_prefix(AS_TEXT)?.chars();
    let icon = rest.next()?;
    rest.next().is_none().then_some(icon)
}

/// The names of all icons, sorted: 64 KB, for tools and tests. The
/// firmware never calls it, so its image is without them (#258).
pub fn names() -> impl Iterator<Item = &'static str> {
    STARTS.iter().zip(&LENGTHS).map(|(&start, &length)| {
        let start = usize::from(start);
        NAMES.get(start..start + usize::from(length)).unwrap_or("")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_are_found_by_name() {
        assert_eq!(char("home"), Some('\u{eac1}'));
        assert!(char("books").is_some());
        assert_eq!(char("no-such-icon"), None);
        assert_eq!(char(""), None);
        // Icons of one's own by their code point.
        assert_eq!(char("U+F8000"), Some('\u{f8000}'));
        assert_eq!(char("U+ZZ"), None);
        assert_eq!(char("U+D800"), None);
        assert_eq!(by_name("home"), char("home"));
    }

    /// What the lookup rests on: each hash once, ascending, and a code
    /// point for each.
    #[test]
    fn the_hashes_ascend_and_each_has_its_code_point() {
        assert!(HASHES.len() > 5000);
        assert_eq!(HASHES.len(), CODES.len());
        assert!(
            HASHES.windows(2).all(|w| w[0] < w[1]),
            "ascending, each once"
        );
        assert_eq!(hash(""), 0x811c_9dc5);
        assert_eq!(hash("a"), 0xe40c_292c);
    }

    /// The names that say what an icon means are no icons' names, nor do
    /// their hashes happen to be an icon's: PaperPaw draws those itself.
    #[test]
    fn what_an_icon_means_is_no_icon() {
        for name in [BACK, PREV, NEXT, HOLD, BUSY, SHIFT, DELETE, SPACE] {
            assert_eq!(char(name), None, "{name}");
        }
        assert!(char(PAW).is_some());
    }
}
