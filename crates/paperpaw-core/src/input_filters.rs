//! What the input filters make of an input, before screens, gestures and key
//! maps: the native `core.input-filter` chain first
//! ([`native_host::input_chain`]), then the interface-6 filters.
use super::*;

impl Kernel {
    /// `event` after the native filters and then the interface-6 ones, or the
    /// effects of an input one of them took, cancelled or ran an action for.
    pub(crate) fn filtered(
        &mut self,
        now: Instant,
        event: InputEvent,
    ) -> Result<InputEvent, Effects> {
        let event = self.native_filtered(now, event)?;
        self.legacy_filtered(now, event)
    }

    /// The interface-6 filters: while a screen guards the device, or a PIN or
    /// a password is entered, only those of its plugin.
    fn legacy_filtered(&mut self, now: Instant, event: InputEvent) -> Result<InputEvent, Effects> {
        let (verdict, delivered) = {
            let asking = self.secret_owner();
            let env = self.shared.env(now);
            self.shared
                .input_filters
                .filter_of(&env, &event, asking.as_deref())
        };
        let event = match verdict {
            InputVerdict::Take => {
                // Taken from the recognizers that follow the finger too.
                if let InputEvent::Pointer(p) = delivered {
                    self.recognizers_lose(&p);
                }
                return Err(self.finish());
            }
            InputVerdict::Run(action) => {
                self.dispatch(action);
                self.run_commands();
                return Err(self.finish());
            }
            InputVerdict::Cancel => {
                self.cancel_input(&delivered);
                self.run_commands();
                return Err(self.finish());
            }
            InputVerdict::Replace(other) => other,
            _ => event,
        };
        Ok(event)
    }

    /// What the native input filters made of `event`: the input to go on with
    /// (as it was, or replaced), or the effects of one that was taken,
    /// cancelled or turned into an action.
    fn native_filtered(&mut self, now: Instant, event: InputEvent) -> Result<InputEvent, Effects> {
        use native_host::input_chain::Native;
        match self.native_input(now, &event) {
            Native::Pass => Ok(event),
            Native::Replaced(other) => Ok(other),
            Native::Taken => {
                if let InputEvent::Pointer(p) = event {
                    self.recognizers_lose(&p);
                }
                Err(self.finish())
            }
            Native::Cancelled => {
                self.cancel_input(&event);
                self.run_commands();
                Err(self.finish())
            }
            Native::Gesture(gesture) => {
                // The filter's gesture is the input: the fingers are let go.
                if let InputEvent::Pointer(p) = event {
                    self.recognizers_lose(&p);
                }
                // As a touch does: the part it is in, and the screen a long
                // press of it belongs to.
                self.pane_touched(gesture.start);
                self.input.pressed_on = self.input_index().map(|index| self.stack[index].serial);
                self.recognized(input::Recognized::Gesture(gesture));
                self.run_commands();
                Err(self.finish())
            }
            Native::Raise(name) => {
                if let InputEvent::Pointer(p) = event {
                    self.recognizers_lose(&p);
                }
                match InputKey::parse(&name) {
                    Some(key) => self.route_key(key),
                    None => self.warn_runtime(format!("a filter raised unknown input `{name}`")),
                }
                self.run_commands();
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
        match self.native_gesture(self.now, &gesture) {
            // A replace that made a key or a finger is not a gesture: the
            // original goes on (the count of refusals told it).
            Native::Pass | Native::Replaced(_) => self.recognized(recognized),
            Native::Gesture(made) => self.recognized(input::Recognized::Gesture(made)),
            Native::Taken | Native::Cancelled => {}
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
