//! The picture behind the page, through the host: the settings that say which
//! picture and how, the request to prepare it, the picture drawn under the page,
//! the notice while it is made, and what is said of a failure. What is decided
//! without the host is `native_backdrop`.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, FrameRect},
        commands::CommandSender,
        fonts::Font,
        image::{ImageId, ImageStyle, PrepareImage, PrepareRoot, TONE_BYTES},
        settings::{self, SettingValue},
        snapshot,
        ui::Notify,
    },
};
use paperpaw_ui_kit::{
    Messages, Paint, Style,
    mem::{SERVICE_BUFFER, buffer},
    paint, text,
};

use crate::native_backdrop::{Drawing, Reason, Rendering};
use crate::native_pick;
use crate::native_picture::{Asked, Recipe};
use crate::settings::{ANCHOR, BRIGHTNESS, CONTRAST, FIT, FOLDER, LIGHTEN, PICTURE, RENDER, ZOOM};

/// The longest path of a request (`command.image_prepare`).
const PATH_MAX: usize = 512;
/// Space for a request's command: the longest path, a tone curve and its fields.
const REQUEST: usize = 1024;
/// Space for the notice's command: its text and the record round it.
const NOTICE: usize = 512;

/// What the settings ask for, on this screen: the chosen picture with the fit,
/// the place, the zoom, the paleness, the brightness and contrast and how it
/// is rendered. `None` where no picture is chosen or its path is too long for a
/// request.
///
/// # Errors
/// [`Error::NoMemory`] and the host's refusals.
pub fn recipe(host: &Host<'_>) -> Result<Option<Recipe>, Error> {
    let mut scratch = buffer(SERVICE_BUFFER)?;
    let name = text_of(host, PICTURE.as_str(), &mut scratch)?;
    if name.is_empty() {
        return Ok(None);
    }
    let folder = text_of(host, FOLDER.as_str(), &mut scratch)?;
    let fit = text_of(host, FIT.as_str(), &mut scratch)?;
    let anchor = text_of(host, ANCHOR.as_str(), &mut scratch)?;
    let zoom = int_of(host, ZOOM.as_str(), &mut scratch)?;
    let lighten = int_of(host, LIGHTEN.as_str(), &mut scratch)?;
    let brightness = text_of(host, BRIGHTNESS.as_str(), &mut scratch)?;
    let contrast = text_of(host, CONTRAST.as_str(), &mut scratch)?;
    let render = text_of(host, RENDER.as_str(), &mut scratch)?;
    let folder = native_pick::folder(&folder);
    let path = if folder.is_empty() {
        name
    } else {
        text::concat(&[folder, "/", &name])?
    };
    if path.len() > PATH_MAX {
        return Ok(None);
    }
    let display = snapshot::capture_snapshot(host)?;
    let asked = Asked {
        path: &path,
        bytes: 0,
        modified: 0,
        fit: &fit,
        anchor: &anchor,
        zoom,
        lighten,
        brightness: &brightness,
        contrast: &contrast,
        render: &render,
    };
    Recipe::new(&asked, display.display_width(), display.display_height())
}

fn text_of(host: &Host<'_>, key: &str, scratch: &mut [u8]) -> Result<String, Error> {
    match settings::read_setting(host, key, scratch)? {
        SettingValue::Text(value) => text::copy(value),
        _ => Ok(String::new()),
    }
}

fn int_of(host: &Host<'_>, key: &str, scratch: &mut [u8]) -> Result<i64, Error> {
    Ok(match settings::read_setting(host, key, scratch)? {
        SettingValue::Int(value) => value,
        _ => 0,
    })
}

/// Asks the host to make the picture at `path` of the library at most `side`
/// pixels on its longest side, turned as its EXIF orientation says and through
/// the `tone` curve where there is one; it answers with `ImageReady` for
/// `cookie`.
///
/// # Errors
/// The host's refusals (too many waiting, no consent, a path too long) and
/// [`Error::NoMemory`].
pub fn ask(
    host: &Host<'_>,
    path: &str,
    cookie: u32,
    (side, tone): (u32, Option<&[u8; TONE_BYTES]>),
) -> Result<(), Error> {
    let mut request = PrepareImage::new(PrepareRoot::Library, path, cookie)
        .side(side)
        .turn();
    if let Some(table) = tone {
        request = request.tone(table);
    }
    // A path of at most 512 bytes, the curve's 256 and the command's own fields.
    let mut scratch = buffer(REQUEST)?;
    CommandSender::new(host, &mut scratch).image_prepare(&request)
}

/// Gives a picture back; a refusal leaves it with the host, which ends it with
/// the plugin.
pub fn release(host: &Host<'_>, image: ImageId) {
    let mut scratch = [0u8; 64];
    CommandSender::new(host, &mut scratch)
        .image_release(image)
        .ok();
}

/// Tells why no picture was made, as interface 6 did.
///
/// # Errors
/// Allocation and host errors.
pub fn tell(host: &Host<'_>, reason: Reason) -> Result<(), Error> {
    let message = Messages::new()?.copy(host, reason.message())?;
    let mut scratch = [0u8; NOTICE];
    CommandSender::new(host, &mut scratch).notify(Notify::new(&message)?)
}

/// Draws the picture under the page: the paper first where the picture does
/// not cover the screen or is paler than itself, then the picture in each of its
/// places. A refusal of the host (an ended picture, the card) leaves the page
/// plain there.
///
/// # Errors
/// Allocation, canvas and font errors.
pub fn draw(
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    style: &Style,
    drawing: &Drawing,
) -> Result<(), Error> {
    if drawing.paper {
        let (width, height) = drawing.screen;
        let paper = [Paint::Fill(Rect::new(0, 0, width, height), style.paper)];
        paint::replay(frame, font, paper.iter())?;
    }
    let mut how = ImageStyle::new().transparency(drawing.transparency);
    if drawing.stretch {
        how = how.stretch();
    }
    how = match drawing.render {
        Rendering::Panel => how,
        Rendering::Dots => how.dots(),
        Rendering::Grays => how.grays(),
    };
    let canvas = frame.draw_canvas()?;
    for place in &drawing.places {
        let rect = FrameRect::new(place.x, place.y, place.width, place.height);
        canvas.draw_image(drawing.image, rect, how).ok();
    }
    Ok(())
}

/// The notice that shows while the picture is made: a title and a few small
/// lines below it, in the middle of the screen.
///
/// # Errors
/// Allocation, font and canvas errors.
pub fn notice(
    host: &Host<'_>,
    frame: &mut DrawFrame<'_>,
    font: &Font<'_>,
    style: &Style,
    screen: (u16, u16),
) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    let title = messages.copy(host, "picture-home.preparing")?;
    let body = messages.copy(host, "picture-home.preparing.text")?;
    let (width, height) = screen;
    let room = f32::from(width.saturating_sub(style.margin.saturating_mul(4))).max(1.0);
    let lines = text::wrap_with(|line| font.measure_text(style.small, line), &body, room, 6)?;
    let head = paperpaw_ui_kit::geometry::line_px(font, style.title)?;
    let line = paperpaw_ui_kit::geometry::line_px(font, style.small)?;
    let total = u32::from(head)
        + u32::from(style.gap)
        + u32::from(line).saturating_mul(u32::try_from(lines.len()).unwrap_or(0));
    let mut y = u32::from(height).saturating_sub(total) / 2;
    let mut out = Vec::new();
    let fitted = text::fit(font, style.title, &title, room)?;
    row(
        &mut out,
        font,
        style.title,
        &fitted,
        (width, y, head),
        style,
    )?;
    y = y.saturating_add(u32::from(head) + u32::from(style.gap));
    for text in &lines {
        row(&mut out, font, style.small, text, (width, y, line), style)?;
        y = y.saturating_add(u32::from(line));
    }
    paint::replay(frame, font, out.iter())
}

/// One centred line of the notice: `(width of the screen, top, height)`.
fn row(
    out: &mut Vec<Paint>,
    font: &Font<'_>,
    size: f32,
    line: &str,
    (width, top, height): (u16, u32, u16),
    style: &Style,
) -> Result<(), Error> {
    let x = (f32::from(width) - font.measure_text(size, line)?).max(0.0) / 2.0;
    let area = Rect::new(0, u16::try_from(top).unwrap_or(u16::MAX), width, height);
    text::label(out, font, line, size, x, area, style.ink, None)
}
