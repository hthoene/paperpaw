//! An address to open on a phone (a page served on the local network, see
//! [`served`](crate::served)): as a QR code, as text, and a hint.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

/// The height of what [`draw_address`] writes below the code in an area
/// `width` wide: the address, `hint` and `last`, all their words. An area
/// as high holds them without the code; the code takes what is more.
pub fn address_height(
    env: &Env<'_>,
    style: &UiStyle,
    width: u16,
    url: &str,
    hint: &str,
    last: Option<&str>,
) -> u16 {
    let Some(font) = style.font(env) else {
        return 0;
    };
    let lines = AddressLines::new(&font, style, width, (url, hint, last), style.small);
    lines.height(&font, style)
}

/// The lines [`draw_address`] writes: the address (on two lines, the
/// host and the rest, where it is too long for one), the hint's and the
/// last thing's, wrapped in `size`.
struct AddressLines<'a> {
    address: Vec<&'a str>,
    hint: Vec<&'a str>,
    last: Vec<&'a str>,
    size: f32,
}

impl<'a> AddressLines<'a> {
    fn new(
        font: &FontHandle<'_>,
        style: &UiStyle,
        width: u16,
        (url, hint, last): (&'a str, &'a str, Option<&'a str>),
        size: f32,
    ) -> Self {
        let width = f32::from(width.saturating_sub(2 * style.margin as u16)).max(1.0);
        let shown = url.strip_prefix("http://").unwrap_or(url);
        let address = match shown.find('/') {
            Some(at) if font.measure(style.text, shown) > width => {
                alloc::vec![&shown[..at], &shown[at..]]
            }
            _ => alloc::vec![shown],
        };
        let wrapped = |text: &'a str| -> Vec<&'a str> {
            font.wrap(size, text, width)
                .into_iter()
                .map(|range| text[range].trim())
                .collect()
        };
        Self {
            address,
            hint: wrapped(hint),
            last: last.map(wrapped).unwrap_or_default(),
            size,
        }
    }

    fn height(&self, font: &FontHandle<'_>, style: &UiStyle) -> u16 {
        let line = font.line_metrics(style.text).line_height as u16;
        let small = font.line_metrics(self.size).line_height as u16;
        let gap = style.gap as u16;
        let last = if self.last.is_empty() {
            0
        } else {
            gap + small * self.last.len() as u16
        };
        gap + line * self.address.len() as u16 + gap + small * self.hint.len() as u16 + last
    }
}

/// `url` as a QR code (`qr`, if it could be made) as large as fits at the
/// top of `area`, below it the address as text (on two lines where it is
/// too long for one, to be typed), `hint` and `last`, what happened last,
/// all their words ([`address_height`]); all centred.
#[allow(
    clippy::too_many_arguments,
    reason = "its inputs are separate things; a struct of them would serve this call alone"
)]
pub fn draw_address(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    style: &UiStyle,
    area: Rect,
    url: &str,
    qr: Option<&crate::qr::QrCode>,
    hint: &str,
    last: Option<&str>,
) {
    let Some(font) = style.font(env) else {
        return;
    };
    let margin = style.margin as u16;
    let line = font.line_metrics(style.text).line_height as u16;
    let width = f32::from(area.width.saturating_sub(2 * margin));
    // The address whole, to be typed; the hint and the last thing wrapped,
    // smaller where the area does not hold them so, down to eight tenths.
    let parts = (url, hint, last);
    let least = style.small * 0.8;
    let size = crate::widgets::largest_size(style.small, least, |size| {
        let lines = AddressLines::new(&font, style, area.width, parts, size);
        lines.height(&font, style) <= area.height
    })
    .unwrap_or(least);
    let lines = AddressLines::new(&font, style, area.width, parts, size);
    let small = font.line_metrics(size).line_height as u16;
    let room = area
        .height
        .saturating_sub(lines.height(&font, style) + style.gap as u16);
    let side = room.min(area.width.saturating_sub(2 * margin));
    let mut y = area.y;
    if let Some(qr) = qr
        && draw_qr(canvas, qr, Rect::new(area.x, y, area.width, side)).is_some()
    {
        y += side;
    }
    y += style.gap as u16;
    let row = |y: u16, height: u16| {
        Rect::new(
            area.x + margin,
            y,
            area.width.saturating_sub(2 * margin),
            height,
        )
    };
    let centered = |text: &str, size: f32| {
        let x = f32::from(area.x + margin) + (width - font.measure(size, text)) / 2.0;
        x.max(f32::from(area.x + margin))
    };
    for part in &lines.address {
        let x = centered(part, style.text);
        draw_label_in(
            canvas,
            &font,
            style,
            TextRole::BODY,
            style.text,
            x,
            row(y, line),
            part,
            Color::BLACK,
        );
        y += line;
    }
    y += style.gap as u16;
    for part in &lines.hint {
        let x = centered(part, lines.size);
        draw_label_in(
            canvas,
            &font,
            style,
            TextRole::BODY,
            lines.size,
            x,
            row(y, small),
            part,
            style.faint(Ink::NOTE),
        );
        y += small;
    }
    if !lines.last.is_empty() {
        y += style.gap as u16;
    }
    for part in &lines.last {
        let x = centered(part, lines.size);
        draw_label_in(
            canvas,
            &font,
            style,
            TextRole::BODY,
            lines.size,
            x,
            row(y, small),
            part,
            Color::BLACK,
        );
        y += small;
    }
}
