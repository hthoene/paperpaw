//! What the sleep screen decides, without the host: what it shows for the
//! setting and the facts at hand, which picture of the folder is next, and
//! where and how opaque a picture over the page goes. The same rules as the
//! interface-6 screen (`lib.rs`) and its overlay (`overlay.rs`), with fallible
//! allocation and the facts given by the caller instead of read from an
//! environment. Decoding, drawing and the host's files are the native
//! screen's (see `PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{Error, image::PrepareRoot};

/// The modes the plugin has itself.
pub const CAT: &str = "sleep.show.cat";
pub const COVER: &str = "sleep.show.cover";
pub const PICTURE: &str = "sleep.show.picture";
pub const MESSAGE: &str = "sleep.show.message";
pub const PAGE: &str = "sleep.show.page";
pub const WIDGETS: &str = "sleep.show.widgets";
pub const OVERLAY: &str = "sleep.show.overlay";
const BUILT_IN: [&str; 7] = [CAT, COVER, PICTURE, OVERLAY, WIDGETS, MESSAGE, PAGE];

/// The slot the sleep screen is shown in, and the plugin's own screen for it.
pub const SLOT: &str = "core.sleep";
pub const OWN_SCREEN: &str = "sleep.screen";

/// Whether `value` of `sleep.show` is another plugin's mode: not one of the
/// seven of the plugin's own and not empty.
#[must_use]
pub fn is_foreign(value: &str) -> bool {
    !value.is_empty() && !BUILT_IN.contains(&value)
}

/// The screen the slot [`SLOT`] is bound to for the stored `value` of
/// `sleep.show`, given the screens now offered for the slot (`candidates`):
/// `Some(screen)` for another plugin's mode that is one of them, `None` (the
/// slot's default, the plugin's own screen) for a mode of the plugin's own, an
/// empty value, the plugin's own screen's id, or a value no plugin offers (a
/// plugin that is gone, a word from a newer version: the host would refuse it,
/// and the screen falls back in [`decide`]).
#[must_use]
pub fn binding<'a, S: AsRef<str>>(value: &str, candidates: &'a [S]) -> Option<&'a str> {
    if !is_foreign(value) || value == OWN_SCREEN {
        return None;
    }
    candidates
        .iter()
        .map(AsRef::as_ref)
        .find(|screen| *screen == value)
}

/// What to do with the slot [`SLOT`] after the reader changed `sleep.show` to
/// `value`. Asked only for such a change, never at start: the slot's binding
/// is the reader's, and a start leaves it alone (`reg_slot.flags` bit 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Bind<'a> {
    /// Bind the slot to this screen (another plugin's mode).
    Screen(&'a str),
    /// Bind the slot back to its default, the plugin's own screen.
    Default,
    /// Touch nothing.
    Leave,
}

/// What to do for the new `value` of `sleep.show` (Kernel's ruling):
/// - another plugin's mode that is one of the screens now offered for the
///   slot (`offered`): bind it;
/// - one of the plugin's own modes, or empty: bind back to the default, but
///   only if the slot is bound now (`current`) to a screen that is one of
///   `sleep.show`'s foreign choices (`choices`), a sleep mode; a screen the
///   reader bound by hand for another purpose is left alone;
/// - a value no plugin offers, or the plugin's own screen's id: leave it.
#[must_use]
pub fn plan<'a, S: AsRef<str>, C: AsRef<str>>(
    value: &str,
    offered: &'a [S],
    current: Option<&str>,
    choices: &[C],
) -> Bind<'a> {
    if let Some(screen) = binding(value, offered) {
        return Bind::Screen(screen);
    }
    if is_foreign(value) {
        return Bind::Leave;
    }
    match current {
        Some(now)
            if is_foreign(now)
                && now != OWN_SCREEN
                && choices.iter().any(|choice| choice.as_ref() == now) =>
        {
            Bind::Default
        }
        _ => Bind::Leave,
    }
}

/// What to do at start and on `plugins-changed` (event 70) for the stored
/// `value`: bind another plugin's mode that is offered now, but only while the
/// slot's binding is not the reader's (`reg_slot.flags` bit 0 clear:
/// `user_bound` false). Never unbinds, and binds nothing for a mode of the
/// plugin's own, an empty value or one nobody offers. The host does not resolve
/// slots from `sleep.show`; this is the plugin's job.
#[must_use]
pub fn plan_stored<'a, S: AsRef<str>>(value: &str, offered: &'a [S], user_bound: bool) -> Bind<'a> {
    if user_bound {
        return Bind::Leave;
    }
    match binding(value, offered) {
        Some(screen) => Bind::Screen(screen),
        None => Bind::Leave,
    }
}

/// What is at hand when the screen opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools, reason = "one fact each")]
pub struct Facts {
    /// The device is switched off after this screen, not put to sleep.
    pub off: bool,
    /// A picture can go over the page now: the setting says it is used and
    /// the picture or widget could be made.
    pub over_page: bool,
    /// Another plugin's screen was made for the mode.
    pub other_screen: bool,
    /// The next picture of the folder could be decoded.
    pub picture: bool,
    /// The cover of the book read last could be found.
    pub cover: bool,
    /// The widgets chosen came up empty (none chosen, none loaded).
    pub widgets_empty: bool,
}

/// What the screen draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Content {
    /// The picture or widget over the page.
    Overlay,
    /// A screen another plugin offers for the mode.
    Other,
    /// A picture of the folder.
    Picture,
    /// The cover, on black.
    Cover,
    /// The sleeping cat, on white.
    Cat,
    /// The widgets.
    Widgets,
    /// PaperPaw's logo and the word *Sleeping*.
    Message,
    /// The page, as it is.
    Page,
}

/// The mode `setting` names, after the two fallbacks: off, a page that
/// stayed would look like a device that is on (the message instead), and an
/// overlay with nothing to show is the cover.
#[must_use]
pub fn effective(setting: &str, facts: Facts) -> &str {
    let mut show = setting;
    if facts.off && matches!(show, PAGE | OVERLAY) {
        show = MESSAGE;
    }
    if show == OVERLAY && !facts.over_page {
        show = COVER;
    }
    show
}

/// What the screen draws for `setting` and the `facts`.
#[must_use]
pub fn decide(setting: &str, facts: Facts) -> Content {
    let show = effective(setting, facts);
    if show == OVERLAY {
        return Content::Overlay;
    }
    let other = !BUILT_IN.contains(&show) && facts.other_screen;
    if other {
        return Content::Other;
    }
    let image = match show {
        OVERLAY | MESSAGE | PAGE | WIDGETS | CAT => None,
        PICTURE => facts.picture.then_some(Content::Picture),
        _ => facts.cover.then_some(Content::Cover),
    };
    if let Some(image) = image {
        return image;
    }
    // The cat, chosen, or where there is no cover or no picture (also for a
    // mode no plugin offers any more, which shows the cover).
    if !matches!(show, MESSAGE | PAGE | WIDGETS) {
        return Content::Cat;
    }
    match show {
        PAGE => Content::Page,
        WIDGETS if !facts.widgets_empty => Content::Widgets,
        _ => Content::Message,
    }
}

/// Whether a file of the pictures folder is one to show: not a folder, not
/// hidden, a JPEG, PNG or GIF by its extension (any case).
#[must_use]
pub fn is_picture(name: &str, is_dir: bool) -> bool {
    !is_dir
        && !name.starts_with('.')
        && [".jpg", ".jpeg", ".png", ".gif"].iter().any(|ext| {
            name.len() >= ext.len()
                && name.is_char_boundary(name.len() - ext.len())
                && name[name.len() - ext.len()..].eq_ignore_ascii_case(ext)
        })
}

/// Pictures kept of a folder at most: the first by name, so that a folder of
/// thousands costs a bounded part of the heap and what is shown does not
/// depend on the order the card lists them in.
pub const MAX_PICTURES: usize = 512;
/// The names of the kept pictures take this many bytes at most.
pub const MAX_NAME_BYTES: usize = 24 * 1024;

/// The listing of the pictures folder, entry by entry: the first
/// [`MAX_PICTURES`] pictures by name, within [`MAX_NAME_BYTES`] of names.
#[derive(Debug, Default)]
pub struct Pictures {
    /// In order of name.
    names: Vec<String>,
    bytes: usize,
}

impl Pictures {
    /// Takes one entry of the folder; pictures only. One that sorts after the
    /// pictures kept, when no more fit, is left out.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; the entry is not taken and nothing changed.
    pub fn take(&mut self, name: &str, is_dir: bool) -> Result<(), Error> {
        if !is_picture(name, is_dir) || name.len() > MAX_NAME_BYTES {
            return Ok(());
        }
        let Err(at) = self.names.binary_search_by(|kept| kept.as_str().cmp(name)) else {
            return Ok(());
        };
        let full = self.names.len() >= MAX_PICTURES
            || self.bytes.saturating_add(name.len()) > MAX_NAME_BYTES;
        if full && at >= self.names.len() {
            return Ok(());
        }
        let name = paperpaw_ui_kit::text::copy(name)?;
        self.names.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.bytes = self.bytes.saturating_add(name.len());
        self.names.insert(at, name);
        while self.names.len() > MAX_PICTURES || self.bytes > MAX_NAME_BYTES {
            let Some(last) = self.names.pop() else { break };
            self.bytes = self.bytes.saturating_sub(last.len());
        }
        Ok(())
    }

    /// The picture after `shown` pictures, one after the other from where the
    /// last sleep left off, and the count to keep for the next time; `None`
    /// for a folder with none.
    pub fn next(&self, shown: usize) -> Option<(&str, usize)> {
        let name = self.names.get(shown.checked_rem(self.names.len())?)?;
        Some((name, shown.wrapping_add(1)))
    }
}

/// How many pictures were shown before, from the kept text (0 for none or
/// for text that is no number).
#[must_use]
pub fn shown_before(kept: &[u8]) -> usize {
    core::str::from_utf8(kept)
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Where a picture over the page goes (`sleep.overlay-position`).
pub const POSITIONS: [&str; 7] = [
    "sleep.position.center",
    "sleep.position.top",
    "sleep.position.bottom",
    "sleep.position.top-left",
    "sleep.position.top-right",
    "sleep.position.bottom-left",
    "sleep.position.bottom-right",
];

/// The screen and its interface's measures, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub width: u16,
    pub height: u16,
    /// The interface's margin.
    pub margin: u16,
    /// The room a status line takes above or below (its height and gap).
    pub status: u16,
}

/// The size setting in percent, held to 10-100.
#[must_use]
pub fn percent(setting: i64) -> u32 {
    setting.clamp(10, 100) as u32
}

/// The side of the square a widget fits in: percent of the shorter side.
#[must_use]
pub fn box_side(screen: Screen, percent: u32) -> u16 {
    (u32::from(screen.width.min(screen.height)) * percent / 100) as u16
}

/// The box a picture fits in: so much of the screen as the size says.
#[must_use]
pub fn box_size(screen: Screen, percent: u32) -> (u16, u16) {
    let part = |side: u16| (u32::from(side) * percent / 100) as u16;
    (part(screen.width), part(screen.height))
}

/// `width` × `height` of a picture of `image` fitted in the box, keeping its
/// proportions.
#[must_use]
pub fn fit(image: (u16, u16), width: u16, height: u16) -> (u16, u16) {
    let (iw, ih) = (u64::from(image.0), u64::from(image.1));
    if iw == 0 || ih == 0 {
        return (0, 0);
    }
    let by_width = u64::from(width) * ih / iw;
    if by_width <= u64::from(height) {
        (width, by_width as u16)
    } else {
        ((u64::from(height) * iw / ih) as u16, height)
    }
}

/// Where something `w` × `h` goes: inside the margins and clear of the
/// status lines, as `position` says; `fill` (a picture at 100 %) has the
/// whole screen to the edges. Returns `(x, y, w, h)`.
#[must_use]
pub fn placed(screen: Screen, w: u16, h: u16, fill: bool, position: &str) -> (u16, u16, u16, u16) {
    let (margin, status) = if fill {
        (0, 0)
    } else {
        (screen.margin, screen.status)
    };
    let w = w.min(screen.width.saturating_sub(margin.saturating_mul(2)));
    let h = h.min(
        screen
            .height
            .saturating_sub(margin.saturating_add(status).saturating_mul(2)),
    );
    let left = margin;
    let right = screen.width.saturating_sub(w.saturating_add(margin));
    let top = margin.saturating_add(status);
    let bottom = screen
        .height
        .saturating_sub(h.saturating_add(margin).saturating_add(status));
    let mid_x = screen.width.saturating_sub(w) / 2;
    let mid_y = screen.height.saturating_sub(h) / 2;
    let (x, y) = match POSITIONS.iter().position(|p| *p == position) {
        Some(0) => (mid_x, mid_y),
        Some(1) => (mid_x, top),
        Some(2) => (mid_x, bottom),
        Some(3) => (left, top),
        Some(4) => (right, top),
        Some(5) => (left, bottom),
        _ => (right, bottom),
    };
    (x, y, w, h)
}

/// How opaque the picture is drawn (0 to 255) for `see_through` percent of
/// the page showing through.
#[must_use]
pub fn opacity(see_through: i64) -> u8 {
    let through = see_through.clamp(0, 100) as u32;
    (255 * (100 - through) / 100) as u8
}

/// The core setting that names a picture shown in place of PaperPaw's logo
/// (a JPEG, PNG or GIF on the card, e.g. a data plugin's
/// `plugins/mine/logo.png`); empty for the plugin's own.
pub const LOGO_SETTING: &str = "core.logo";
/// The plugin's own logo, as written into its data folder.
pub const OWN_LOGO: &str = "logo.png";
/// The longest path a picture is asked for with (`command.image_prepare`).
pub const LOGO_PATH_MAX: usize = 512;

/// The logo picture the message asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogoFile<'a> {
    /// The plugin's own ([`OWN_LOGO`] in its data folder).
    Own,
    /// The card file [`LOGO_SETTING`] names, from the card's root.
    Card(&'a str),
}

impl<'a> LogoFile<'a> {
    /// Where the host reads it and its path there: the data folder for the
    /// plugin's own, the card (the library root) for the one set.
    #[must_use]
    pub fn request(self) -> (PrepareRoot, &'a str) {
        match self {
            LogoFile::Own => (PrepareRoot::Data, OWN_LOGO),
            LogoFile::Card(path) => (PrepareRoot::Library, path),
        }
    }
}

/// The logo to ask for: the picture `setting` (the value of
/// [`LOGO_SETTING`]) names, without spaces and slashes at its ends as
/// interface 6 read it, unless that one could not be had (`card_failed`: the
/// host refused it or could not decode it); the plugin's own where none is
/// set, it failed or its path is too long for a request.
#[must_use]
pub fn logo_file(setting: &str, card_failed: bool) -> LogoFile<'_> {
    let path = setting.trim().trim_matches('/');
    if card_failed || path.is_empty() || path.len() > LOGO_PATH_MAX {
        return LogoFile::Own;
    }
    LogoFile::Card(path)
}

#[cfg(test)]
mod tests;
