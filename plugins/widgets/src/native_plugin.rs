//! The widget pack on the flat native boundary: the host makes an instance
//! when a screen shows a widget, asks it for its height and where it sits, and
//! calls the plugin to draw it. Nothing of a callback is kept; an instance is
//! the widget's id with its placement and the rectangle it was placed in.
use crate::native_cx::Cx;
use crate::native_defs::{behaviour, token_of};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::CommandSender,
        events::Event,
        registrations,
        reply::Reply,
        ui::{Instance, stages::WidgetCall},
    },
};
use paperpaw_ui_kit::paint;

mod image {
    include!(concat!(env!("OUT_DIR"), "/widgets_declarations.rs"));
}

/// The callback token of the minute tick, as declared.
const TICK: u32 = crate::declarations::TICK;
/// The most bytes of a stage reply (sizes and up to 32 targets).
const REPLY: usize = 1280;

#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when it was last drawn or asked about, for the tick.
    shown: Option<String>,
}

/// The widget pack.
#[derive(Debug, Default)]
pub struct NativeWidgets {
    items: Vec<Item>,
}

impl NativeWidgets {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let Some(token) = token_of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: paperpaw_ui_kit::text::copy(widget)?,
            shown: None,
        });
        Ok(())
    }

    fn stage(&self, host: &Host<'_>, payload: &[u8], reply: &mut Reply<'_>) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = [0; REPLY];
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let height = (behaviour.height)(&mut cx, u16::try_from(width).unwrap_or(u16::MAX))?;
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_targets(&[], &mut out)?
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// Tells the makers whose widget shows something new since the last minute.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let Some(behaviour) = behaviour(item.token) else {
                continue;
            };
            let cx = Cx::open(host, &item.widget)?;
            let now = (behaviour.shows)(&cx)?;
            let changed = item.shown.as_deref().is_some_and(|before| before != now);
            item.shown = Some(now);
            if changed {
                commands.widget_changed(item.instance)?;
            }
        }
        Ok(())
    }
}

impl Plugin for NativeWidgets {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::WIDGETS.iter().chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::WidgetMade {
                widget, instance, ..
            } => self.made(widget, instance),
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::Tick { .. } if token == TICK => self.tick(host),
            _ => Ok(()),
        }
    }

    fn draw(
        &mut self,
        host: &Host<'_>,
        _token: u32,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = Rect::new(
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.x)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.y)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.width)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.height)),
        );
        let mut out = Vec::new();
        (behaviour.draw)(&mut cx, &mut out, area)?;
        paint::replay(&mut frame, &cx.font, &out)
    }

    fn input(&mut self, _: &Host<'_>, _: u32, _: Input<'_>) -> Result<Handled, Error> {
        Ok(Handled::No)
    }
}
