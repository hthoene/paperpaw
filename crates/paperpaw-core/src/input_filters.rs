//! What the input filters make of an input, before screens, gestures and key
//! maps: the native `core.input-filter` chain
//! ([`native_host::input_chain`]).
use super::*;

impl Kernel {
    /// `event` after the native filters, or the effects of an input one of
    /// them took, cancelled or ran an action for.
    pub(crate) fn filtered(
        &mut self,
        now: Instant,
        event: InputEvent,
    ) -> Result<InputEvent, Effects> {
        self.native_filtered(now, event)
    }

    /// What the native input filters made of `event`: the input to go on with
    /// (as it was, or replaced), or the effects of one that was taken,
    /// cancelled or turned into an action.
    fn native_filtered(&mut self, now: Instant, event: InputEvent) -> Result<InputEvent, Effects> {
        use native_host::input_chain::Native;
        let verdict = self.native_input(now, &event);
        // A release or a lift that does not go on ends the press for the
        // kernel too: followed on, the button would fire a long press of its
        // own later and swallow its next press (#483).
        if !matches!(
            verdict,
            Native::Pass | Native::Replaced(_) | Native::Cancelled
        ) && ends_a_press(&event)
        {
            self.cancel_input(&event);
        }
        match verdict {
            Native::Pass => Ok(event),
            Native::Replaced(other) => Ok(other),
            Native::Taken => Err(self.finish()),
            Native::Cancelled => {
                self.cancel_input(&event);
                self.run_commands();
                Err(self.finish())
            }
            Native::Gesture(gesture) => {
                // The filter's gesture is the input. As a touch does: the part it is in, and the screen a long
                // press of it belongs to.
                self.input.pressed_on = self.input_index().map(|index| self.stack[index].serial);
                self.recognized(input::Recognized::Gesture(gesture));
                self.run_commands();
                Err(self.finish())
            }
            Native::Raise(name) => {
                match InputKey::parse(&name) {
                    Some(key) => self.route_key(key),
                    None => self.warn_runtime(format!("a filter raised unknown input `{name}`")),
                }
                self.run_commands();
                Err(self.finish())
            }
            Native::Preview {
                owner,
                key,
                value,
                begun_mid,
            } => {
                if begun_mid {
                    self.cancel_input(&event);
                }
                self.filter_preview(owner, &key, &value);
                Err(self.finish())
            }
            Native::Run(action) => match Id::parse(&action) {
                Ok(action) => {
                    self.dispatch(action);
                    self.run_commands();
                    Err(self.finish())
                }
                Err(_) => Ok(event),
            },
        }
    }

    /// Shows the value a filter's verdict `preview` asked for in place of the
    /// setting's own, as `command.preview_setting` would, until its capture
    /// ends ([`Self::end_filter_preview`]) or another preview is set. A row
    /// that is not accepted shows nothing (the input is taken all the same).
    fn filter_preview(&mut self, owner: u64, key: &str, value: &[u8]) {
        match self.filter_preview_row(owner, key, value) {
            Ok((id, shown)) => {
                self.preview_settings(alloc::vec![(id, Some(shown))]);
                self.shared.filter_preview = true;
            }
            Err(_) => self.warn_runtime("a filter's preview was refused"),
        }
    }

    /// Ends the preview a filter's capture showed (not another one's).
    pub(crate) fn end_filter_preview(&mut self) {
        if self.shared.filter_preview {
            self.preview_settings(Vec::new());
        }
    }

    /// What the recognizer made of the input, through the native filters first
    /// where it is a gesture an `input.gesture` record can describe (a tap, a
    /// long press, a swipe): they may pass it, replace it by another gesture,
    /// take or cancel it, run an action or raise an input of their plugin's.
    /// A gesture a filter made, or a raised input, goes on without them
    /// ([`Self::recognized`]), so none can loop.
    pub(crate) fn recognized_filtered(&mut self, recognized: input::Recognized) {
        use native_host::input_chain::Native;
        let input::Recognized::Gesture(gesture) = recognized else {
            return self.recognized(recognized);
        };
        match self.native_gesture(self.shared.now, &gesture) {
            // A replace that made a key or a finger is refused (counted by the
            // chain) and passes the original; `Replaced` does not come here.
            Native::Pass | Native::Replaced(_) => self.recognized(recognized),
            Native::Gesture(made) => {
                // As a touch does (see `native_filtered`): the part it is in
                // and the screen a long press of it belongs to.
                self.input.pressed_on = self.input_index().map(|index| self.stack[index].serial);
                self.recognized(input::Recognized::Gesture(made));
            }
            // Taken or cancelled, a recognized gesture is only dropped: the
            // finger's input was recognized (and passed) already, so unlike a
            // cancelled key or pointer there is nothing more of it to drop.
            Native::Taken | Native::Cancelled | Native::Preview { .. } => {}
            Native::Run(action) => {
                if let Ok(action) = Id::parse(&action) {
                    self.dispatch(action);
                }
            }
            Native::Raise(name) => match InputKey::parse(&name) {
                Some(key) => self.route_key(key),
                None => self.warn_runtime(format!("a filter raised unknown input `{name}`")),
            },
        }
    }
}

/// Whether `event` lets a button go or lifts a finger.
fn ends_a_press(event: &InputEvent) -> bool {
    match event {
        InputEvent::Button { state, .. } => *state == ButtonState::Released,
        InputEvent::Pointer(p) => matches!(p.phase, PointerPhase::Up | PointerPhase::Cancel),
        _ => false,
    }
}
