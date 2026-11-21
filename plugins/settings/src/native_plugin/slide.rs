//! The sliders of the number rows on a touch screen: a finger on a track moves
//! the knob as far as it moves itself, the number shows at once in its row
//! alone (a quick refresh) and is previewed, and lifting the finger keeps it. A
//! tap on the track sets the number there.
use super::{Entry, GroupView, Handled, Messages, NativeSettings, View, copy, set_value};
use paperpaw_sdk::{
    Point,
    native::{
        Error, Host,
        callbacks::{Input, InputGestureKind, PointerPhase},
        commands::{CommandSender, PreviewSetting},
        settings::SettingValue,
    },
};
use paperpaw_ui_kit::{
    mem::buffer,
    slider::{self, Drag},
};

impl NativeSettings {
    /// What a finger on a slider's track does; `None` for input that is not
    /// for a slider.
    pub(super) fn slide_input(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Some(View::Group(v)) = self.screens.top(token) else {
            return Ok(None);
        };
        match *input {
            Input::Pointer {
                position,
                phase,
                contact,
                ..
            } => Self::on_pointer(host, (v, &mut self.drag), (contact, position), phase),
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } if v.touch => match v.list.track_at(position) {
                Some((index, track)) => {
                    Self::set_by_tap(host, v, (index, track), position.x)?;
                    Ok(Some(Handled::Yes))
                }
                None => Ok(None),
            },
            _ => Ok(None),
        }
    }

    fn on_pointer(
        host: &Host<'_>,
        (v, held): (&mut GroupView, &mut Option<Drag>),
        (contact, at): (u32, Point),
        phase: PointerPhase,
    ) -> Result<Option<Handled>, Error> {
        let on = held.filter(|d| d.contact == contact);
        match (phase, on) {
            (PointerPhase::Down, _) if v.touch => {
                *held = None;
                if let Some((index, track)) = v.list.track_at(at) {
                    *held = Self::begin(v, (contact, index), track, at.x);
                }
                // A finger taken is the slider's alone, up to its lifting.
                Ok(held.is_some().then_some(Handled::Yes))
            }
            (PointerPhase::Move, Some(mut drag)) => {
                let moved = drag.follow(at.x);
                *held = Some(drag);
                if let Some(value) = moved {
                    Self::show(host, v, drag.row, value)?;
                }
                Ok(Some(Handled::Yes))
            }
            (PointerPhase::Up, Some(mut drag)) => {
                *held = None;
                if !drag.moved() {
                    // A tap on the track: the number there.
                    Self::set_by_tap(host, v, (drag.row, drag.track()), at.x)?;
                    return Ok(Some(Handled::Yes));
                }
                drag.follow(at.x);
                Self::keep(host, v, &drag)?;
                Ok(Some(Handled::Yes))
            }
            (PointerPhase::Cancel, Some(drag)) => {
                *held = None;
                if drag.moved() {
                    CommandSender::new(host, &mut [0; 64]).end_preview()?;
                    v.rebuild_items(host)?;
                    Self::redraw(host)?;
                }
                Ok(Some(Handled::Yes))
            }
            _ => Ok(None),
        }
    }

    /// The knob taken where a finger came down on `track` of row `index`.
    fn begin(
        v: &GroupView,
        (contact, index): (u32, usize),
        track: paperpaw_sdk::Rect,
        x: i32,
    ) -> Option<Drag> {
        let (entry, now) = Self::slider_of(v, index)?;
        let range = (entry.detail.min, entry.detail.max, entry.detail.step);
        Some(Drag::begin(
            (contact, index),
            track,
            range,
            x,
            now.clamp(range.0, range.1),
        ))
    }

    /// The entry of slider row `index` and its number.
    fn slider_of(v: &GroupView, index: usize) -> Option<(&Entry, i64)> {
        let entry = v.entry(index)?;
        let Some(super::Val::Int(now)) = entry.setting.value else {
            return None;
        };
        v.list
            .items()
            .get(index)
            .is_some_and(|item| matches!(item, super::Item::Row(r) if r.track().is_some()))
            .then_some((entry, now))
    }

    /// The number dragged to: previewed (what it changes shows at once) and
    /// shown in its row, which alone is drawn again, quickly.
    fn show(host: &Host<'_>, v: &mut GroupView, index: usize, value: i64) -> Result<(), Error> {
        let Some(entry) = v.entry(index) else {
            return Ok(());
        };
        let key = copy(&entry.setting.key)?;
        let text = Messages::new()?.with_unit(host, value, &entry.detail.unit)?;
        let mut scratch = buffer(1024)?;
        let mut encoded = [0; 64];
        CommandSender::new(host, &mut scratch).preview_settings(
            &[PreviewSetting::new(&key, SettingValue::Int(value))],
            &mut encoded,
        )?;
        v.list.set_number(index, text, value);
        v.list.redraw_row_fast(host, index)
    }

    /// The finger lifted off a knob it moved: the number is kept.
    fn keep(host: &Host<'_>, v: &mut GroupView, drag: &Drag) -> Result<(), Error> {
        let Some(entry) = v.entry(drag.row) else {
            return Ok(());
        };
        let key = copy(&entry.setting.key)?;
        CommandSender::new(host, &mut [0; 64]).end_preview()?;
        set_value(host, &key, &super::Val::Int(drag.value))?;
        Self::redraw(host)
    }

    /// A tap on a slider's track: the number is the nearest step to it.
    fn set_by_tap(
        host: &Host<'_>,
        v: &mut GroupView,
        (index, track): (usize, paperpaw_sdk::Rect),
        x: i32,
    ) -> Result<(), Error> {
        let Some((entry, now)) = Self::slider_of(v, index) else {
            return Ok(());
        };
        let range = (entry.detail.min, entry.detail.max, entry.detail.step);
        let next = slider::value_at(track, x, range);
        if next != now {
            let key = copy(&entry.setting.key)?;
            set_value(host, &key, &super::Val::Int(next))?;
            Self::redraw(host)?;
        }
        Ok(())
    }
}
