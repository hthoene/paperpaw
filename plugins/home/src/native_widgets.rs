//! The home screen's own widgets on the flat native boundary: the host makes an
//! instance when a screen shows one, asks it for its height and where it sits,
//! and calls the plugin to draw it. Nothing of a callback is kept; an instance
//! is the widget's id with its placement. The widgets are the date and the
//! clock; the status line, the continue card and the recent shelf follow
//! (`PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        commands::CommandSender,
        fonts::{Font, FontRole},
        reply::Reply,
        ui::{Instance, WidgetTarget, stages::WidgetCall},
    },
};
use paperpaw_ui_kit::{mem::buffer, paint, text::copy, widget::Cx};

use crate::declarations::{CLOCK, DATE};
use crate::{native_clock, native_date};

/// The most bytes of a stage reply (sizes and no targets).
const REPLY: usize = 256;

/// The widget an instance is.
#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when it was last drawn or asked about, for the tick.
    shown: Option<u64>,
}

/// The instances the screens made of the widgets.
#[derive(Debug, Default)]
pub struct Widgets {
    items: Vec<Item>,
}

/// The token of the widget the host calls `widget` (`home.date`, or placed
/// `home.date@2`), if it is one of ours here.
fn token_of(widget: &str) -> Option<u32> {
    match paperpaw_ui_kit::widget::settings::base(widget) {
        "home.date" => Some(DATE),
        "home.clock" => Some(CLOCK),
        _ => None,
    }
}

impl Widgets {
    /// A screen made an instance of `widget`; one that is none of ours is none
    /// of our business.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let Some(token) = token_of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: copy(widget)?,
            shown: None,
        });
        Ok(())
    }

    /// An instance is gone.
    pub fn dropped(&mut self, instance: Instance) {
        self.items.retain(|item| item.instance != instance);
    }

    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    /// Answers a call of the `core.widget` stage: the height of an instance, where it
    /// sits, its targets (none: a tap on these does nothing).
    ///
    /// # Errors
    /// [`Error::NotFound`] for an instance that is not ours here, [`Error::Unsupported`]
    /// for a call it does not answer; font and host refusals.
    pub fn stage(
        &self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let height = match item.token {
                    DATE => native_date::height(&mut cx, width)?,
                    _ => native_clock::height(&mut cx, width)?,
                };
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                let none: [WidgetTarget; 0] = [];
                call.reply_targets(&none, &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Draws the instance the frame is for.
    ///
    /// # Errors
    /// [`Error::NotFound`] for an instance that is not ours here; font and host
    /// refusals, [`Error::NoMemory`].
    pub fn draw(&self, host: &Host<'_>, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let clamp = paperpaw_ui_kit::geometry::clamp_u16;
        let area = Rect::new(
            clamp(i64::from(frame.rect.x)),
            clamp(i64::from(frame.rect.y)),
            clamp(i64::from(frame.rect.width)),
            clamp(i64::from(frame.rect.height)),
        );
        let mut out = Vec::new();
        match item.token {
            DATE => native_date::draw(&mut cx, &mut out, area)?,
            _ => native_clock::draw(&mut cx, &mut out, area)?,
        }
        let bold = if out
            .iter()
            .any(|op| matches!(op, paperpaw_ui_kit::Paint::Bold { .. }))
        {
            Some(Font::acquire_role(host, FontRole::UiBold)?)
        } else {
            None
        };
        paint::replay_with(&mut frame, &cx.font, bold.as_ref(), &out)
    }

    /// The minute came: tells the makers whose widget shows something new since
    /// the last minute. A widget that fails (its time or its command) is left for
    /// the next tick and costs the others nothing.
    pub fn tick(&mut self, host: &Host<'_>) {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let now = match item.token {
                DATE => native_date::shows(host),
                _ => native_clock::shows(host),
            };
            let Ok(now) = now else {
                continue;
            };
            let instance = item.instance;
            if item.shown.is_some_and(|before| before != now)
                && commands.widget_changed(instance).is_err()
            {
                continue;
            }
            item.shown = Some(now);
        }
    }
}
