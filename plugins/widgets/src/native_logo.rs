//! PaperPaw's logo, its name beside it if wanted, on interface 7.
//!
//! The logo is a picture the host decodes and draws (`docs/abi/document.md` 5):
//! the one the setting `core.logo` names (a JPEG, PNG or GIF on the card), else
//! PaperPaw's own, installed with the plugin (`pictures/logo.png`, read from the
//! plugin's install folder). One request is open at a time and one picture is
//! held, shared by every logo widget. Until it is ready, or where there is none
//! (the host cannot read it), only the name shows, if wanted.
//!
//! What to ask next and what an answer means need no host: [`Logo`] decides,
//! `native_plugin` sends the commands and draws the picture.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        image::{ImageId, ImageStatus, PrepareRoot},
    },
};
use paperpaw_ui_kit::{
    Paint,
    geometry::{clamp_u16, line_px},
    widget::{Align, Cx, TextSize, cx::face_font, layout::label},
};

/// The widget's token, as declared.
pub const TOKEN: u32 = 121;
/// The setting that names the picture shown in place of PaperPaw's own.
pub const SETTING: &str = "core.logo";
/// PaperPaw's own logo in the plugin's install folder.
const BUNDLED: &str = "pictures/logo.png";
/// The name beside the logo: a name, not translated.
const PAPERPAW: &str = "PaperPaw";
/// The longest path of a request (`command.image_prepare`).
pub const PATH_MAX: usize = 512;
/// The longest side the host decodes the logo to.
pub const SIDE: u32 = 320;
/// What the cookies of the logo's requests begin with: the rest is the
/// generation and the step.
const COOKIE: u32 = 0x4c4f_0000;

/// How many files there are to try: the picture set, then PaperPaw's own.
#[must_use]
pub fn steps(own: &str) -> usize {
    1 + usize::from(!own.is_empty())
}

/// Where the `step`-th file to try is. `own` is the picture set (without
/// slashes at its ends; empty: none). `None` past the last file and for a path
/// too long for a request: that file is left out.
#[must_use]
pub fn candidate(own: &str, step: usize) -> Option<(PrepareRoot, &str)> {
    if step >= steps(own) {
        return None;
    }
    if !own.is_empty() && step == 0 {
        return (own.len() <= PATH_MAX).then_some((PrepareRoot::Library, own));
    }
    Some((PrepareRoot::Bundle, BUNDLED))
}

/// Whether `cookie` is one of the logo's requests (of any generation).
#[must_use]
pub const fn is_cookie(cookie: u32) -> bool {
    cookie & 0xffff_0000 == COOKIE
}

/// What the host's answer to the request in flight leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Not the open request's answer (or none open).
    Ignore,
    /// The picture is ready; the one held before (if any) is to be released.
    Ready {
        image: ImageId,
        release: Option<ImageId>,
    },
    /// Not this file: ask the next step.
    Again(usize),
    /// Refused for want of memory or the card: asked again at the next minute.
    Retry,
    /// Nothing more to try: the name stays alone.
    Gone,
}

/// The logo picture and what is asked for it.
#[derive(Debug, Default)]
pub struct Logo {
    /// The picture held, and its size as the host made it.
    held: Option<(ImageId, (u32, u32))>,
    /// The step of the request open.
    open: Option<usize>,
    /// No file could be read (until the setting changes).
    none: bool,
    /// Counts the changes of the setting, so that the answer to a request made
    /// before one is not taken for the answer to the one made after.
    generation: u8,
}

impl Logo {
    /// The picture to draw and its size, if held.
    #[must_use]
    pub const fn held(&self) -> Option<(ImageId, (u32, u32))> {
        self.held
    }

    /// Whether to start asking: not when it is held, asked for or known to be
    /// missing.
    #[must_use]
    pub const fn wants(&self) -> bool {
        self.held.is_none() && self.open.is_none() && !self.none
    }

    /// The number the request for step `step` is made with.
    #[must_use]
    pub const fn cookie(&self, step: usize) -> u32 {
        COOKIE | (self.generation as u32) << 8 | (step as u32 & 0xff)
    }

    /// The request for step `step` is made.
    pub fn asked(&mut self, step: usize) {
        self.open = Some(step);
    }

    /// No file is left to ask for.
    pub fn gave_up(&mut self) {
        self.none = true;
    }

    /// The setting changed: what was found (or not) may differ now. The picture
    /// held, if any, is to be released by the caller. An answer to a request
    /// made before is not the one waited for any more.
    pub fn forget(&mut self) -> Option<ImageId> {
        self.none = false;
        self.open = None;
        self.generation = self.generation.wrapping_add(1);
        self.held.take().map(|(image, _)| image)
    }

    /// The host's answer: its cookie, status, picture and size. `own` says
    /// whether a picture is set (which steps there are).
    pub fn ready(
        &mut self,
        (cookie_of, status, image, size): (u32, ImageStatus, Option<ImageId>, (u32, u32)),
        own: &str,
    ) -> Next {
        let Some(step) = self.open else {
            return Next::Ignore;
        };
        if cookie_of != self.cookie(step) {
            return Next::Ignore;
        }
        self.open = None;
        match (status, image) {
            (ImageStatus::Ready, Some(image)) => {
                let release = self.held.map(|(held, _)| held);
                self.held = Some((image, size));
                Next::Ready { image, release }
            }
            (ImageStatus::NoMemory | ImageStatus::CardFailure, _) => Next::Retry,
            _ => {
                let next = step.saturating_add(1);
                if next < steps(own) {
                    Next::Again(next)
                } else {
                    self.none = true;
                    Next::Gone
                }
            }
        }
    }
}

/// The size of a picture `size` shown as large as fits in a square `side`
/// pixels across, its proportions kept; nothing for an empty picture.
#[must_use]
pub fn fit(size: (u32, u32), side: u16) -> (u16, u16) {
    let (w, h) = (u64::from(size.0), u64::from(size.1));
    let side = u64::from(side);
    if w == 0 || h == 0 || side == 0 {
        return (0, 0);
    }
    let (fw, fh) = if w >= h {
        (side, (side * h / w).max(1))
    } else {
        ((side * w / h).max(1), side)
    };
    (
        clamp_u16(i64::try_from(fw).unwrap_or(i64::MAX)),
        clamp_u16(i64::try_from(fh).unwrap_or(i64::MAX)),
    )
}

/// What the settings make drawn change, as a number: the font of the name.
///
/// # Errors
/// Host refusals.
pub fn shows(host: &Host<'_>, widget: &str) -> Result<u64, Error> {
    let settings = paperpaw_ui_kit::widget::Settings::new(host, widget);
    let face = settings.string("face", 96)?;
    Ok(crate::native_draw::fnv(1, face.as_bytes()))
}

/// The size of the name, and the side of the logo: as high as a line and a half
/// of the name; as large as its place allows where the size is "fit".
fn sizes(cx: &Cx<'_>) -> Result<(f32, u16), Error> {
    let chosen = cx.settings.size()?;
    let size = chosen.text(cx.style());
    let side = match chosen {
        TextSize::Fit => u16::MAX,
        _ => {
            let line = u32::from(line_px(&cx.font, size)?);
            clamp_u16(i64::from(line * 3 / 2))
        }
    };
    Ok((size, side))
}

/// The height: as high as wide, a third of the screen at most, where the size
/// is "fit"; else a line and a half of the name; and the frame.
///
/// # Errors
/// Font and host refusals.
pub fn height(cx: &mut Cx<'_>, width: u16) -> Result<u16, Error> {
    let side = if cx.settings.size()? == TextSize::Fit {
        width.min(cx.snapshot.display_height() / 3)
    } else {
        sizes(cx)?.1
    };
    Ok(side.saturating_add(cx.shape.frame_height()))
}

/// Draws the widget in `area` as far as paint goes (the frame and the name) and
/// says where the picture `held` goes: the host draws it over the paint.
///
/// # Errors
/// Font and host refusals, [`Error::NoMemory`].
pub fn draw_with(
    cx: &mut Cx<'_>,
    out: &mut Vec<Paint>,
    area: Rect,
    held: Option<(ImageId, (u32, u32))>,
) -> Result<Option<(ImageId, Rect)>, Error> {
    let look = cx.shape.paint(out, area, 256)?;
    let area = look.area;
    let (size, most) = sizes(cx)?;
    let side = most.min(area.height).min(area.width);
    let (w, h) = held.map_or((0, 0), |(_, picture)| fit(picture, side));
    let face = cx.settings.string("face", 96)?;
    let font = face_font(cx.host, &face)?;
    let name = if cx.settings.shows("name")? {
        Some(font.measure_text(size, PAPERPAW)?)
    } else {
        None
    };
    // The name a third of the logo's width after it, where it fits.
    let gap = w / 3;
    let beside =
        name.filter(|wide| f32::from(w.saturating_add(gap)) + wide <= f32::from(area.width));
    let total = match beside {
        Some(wide) if w > 0 => f32::from(w.saturating_add(gap)) + wide,
        Some(wide) => wide,
        None => f32::from(w),
    };
    let align: Align = cx.settings.align()?;
    let x = align.x(area, total);
    let tall = if h > 0 { h } else { side };
    let top = area.y.saturating_add(area.height.saturating_sub(tall) / 2);
    let left = clamp_u16(x as i64);
    if beside.is_some() {
        let after =
            clamp_u16(i64::from(left) + i64::from(if w > 0 { w.saturating_add(gap) } else { 0 }));
        let row = Rect::new(
            after,
            top,
            area.x.saturating_add(area.width).saturating_sub(after),
            tall,
        );
        label(out, &font, (size, Align::Left), row, PAPERPAW, look.ink)?;
    }
    Ok(held
        .filter(|_| w > 0 && h > 0)
        .map(|(image, _)| (image, Rect::new(left, top, w, h))))
}

#[cfg(test)]
mod tests;
